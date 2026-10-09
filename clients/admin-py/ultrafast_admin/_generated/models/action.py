from enum import StrEnum


class Action(StrEnum):
    BLOCK = "block"
    FLAG = "flag"
    REDACT = "redact"

    def __str__(self) -> str:
        return str(self.value)
