from enum import StrEnum


class TargetState(StrEnum):
    CLOSED = "closed"
    HALF_OPEN = "half_open"
    OPEN = "open"

    def __str__(self) -> str:
        return str(self.value)
