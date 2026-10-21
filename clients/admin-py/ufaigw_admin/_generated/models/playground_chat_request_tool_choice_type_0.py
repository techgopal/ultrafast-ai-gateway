from enum import StrEnum


class PlaygroundChatRequestToolChoiceType0(StrEnum):
    AUTO = "auto"
    NONE = "none"
    REQUIRED = "required"

    def __str__(self) -> str:
        return str(self.value)
