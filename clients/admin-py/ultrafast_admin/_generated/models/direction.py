from enum import StrEnum


class Direction(StrEnum):
    INPUT = "input"
    OUTPUT = "output"

    def __str__(self) -> str:
        return str(self.value)
