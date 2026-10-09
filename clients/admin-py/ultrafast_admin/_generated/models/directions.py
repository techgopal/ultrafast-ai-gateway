from enum import StrEnum


class Directions(StrEnum):
    BOTH = "both"
    INPUT = "input"
    OUTPUT = "output"

    def __str__(self) -> str:
        return str(self.value)
