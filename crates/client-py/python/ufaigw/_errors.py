"""Errors. The extension builds them through `make`, so the classes live here."""

from __future__ import annotations

from typing import Optional


class Error(Exception):
    """Base of every error the client raises about a call.

    `kind` is one of auth, permission, not_found, invalid_request, rate_limited,
    upstream, network, timeout, malformed. `retryable` says whether the same call
    may succeed later; `retry_after` is the server's wait in whole seconds, if it
    sent one. Messages never contain the API key.
    """

    kind = "error"

    def __init__(
        self,
        message: str,
        *,
        status: Optional[int] = None,
        retryable: bool = False,
        retry_after: Optional[int] = None,
    ):
        super().__init__(message)
        self.message = message
        self.status = status
        self.retryable = retryable
        self.retry_after = retry_after

    def __str__(self) -> str:
        return self.message

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}(kind={self.kind!r}, message={self.message!r}, "
            f"status={self.status!r}, retryable={self.retryable!r}, "
            f"retry_after={self.retry_after!r})"
        )

    def __reduce__(self):
        return (_rebuild, (self.kind, self.message, self.status, self.retryable, self.retry_after))


class AuthenticationError(Error):
    kind = "auth"


class PermissionDeniedError(Error):
    kind = "permission"


class NotFoundError(Error):
    kind = "not_found"


class InvalidRequestError(Error):
    kind = "invalid_request"


class RateLimitError(Error):
    kind = "rate_limited"


class UpstreamError(Error):
    kind = "upstream"


class NetworkError(Error):
    kind = "network"


class RequestTimeoutError(Error):
    kind = "timeout"


class MalformedError(Error):
    kind = "malformed"


_BY_KIND = {
    cls.kind: cls
    for cls in (
        AuthenticationError,
        PermissionDeniedError,
        NotFoundError,
        InvalidRequestError,
        RateLimitError,
        UpstreamError,
        NetworkError,
        RequestTimeoutError,
        MalformedError,
    )
}


def _rebuild(kind, message, status, retryable, retry_after):
    return make(kind, message, status, retryable, retry_after)


def make(kind, message, status, retryable, retry_after):
    """Called by the extension."""
    cls = _BY_KIND.get(kind, Error)
    return cls(message, status=status, retryable=retryable, retry_after=retry_after)
