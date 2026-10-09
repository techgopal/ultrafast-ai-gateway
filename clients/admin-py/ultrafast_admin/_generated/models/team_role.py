from enum import StrEnum


class TeamRole(StrEnum):
    LEAD = "lead"
    MEMBER = "member"

    def __str__(self) -> str:
        return str(self.value)
