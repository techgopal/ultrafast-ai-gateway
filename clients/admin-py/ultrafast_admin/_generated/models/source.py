from enum import StrEnum


class Source(StrEnum):
    BUILTIN = "builtin"
    CACHE = "cache"
    REMOTE = "remote"

    def __str__(self) -> str:
        return str(self.value)
