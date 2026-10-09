from enum import StrEnum


class DatabaseKind(StrEnum):
    POSTGRES = "postgres"
    SQLITE = "sqlite"

    def __str__(self) -> str:
        return str(self.value)
