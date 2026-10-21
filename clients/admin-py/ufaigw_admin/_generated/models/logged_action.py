from enum import StrEnum


class LoggedAction(StrEnum):
    BLOCKED = "blocked"
    FLAGGED = "flagged"
    REDACTED = "redacted"

    def __str__(self) -> str:
        return str(self.value)
