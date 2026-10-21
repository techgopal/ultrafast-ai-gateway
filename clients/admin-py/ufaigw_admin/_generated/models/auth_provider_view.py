from enum import StrEnum


class AuthProviderView(StrEnum):
    OIDC = "oidc"
    PASSWORD = "password"

    def __str__(self) -> str:
        return str(self.value)
