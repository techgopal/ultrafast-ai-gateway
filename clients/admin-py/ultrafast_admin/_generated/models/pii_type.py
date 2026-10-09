from enum import StrEnum


class PiiType(StrEnum):
    CREDIT_CARD = "CREDIT_CARD"
    EMAIL = "EMAIL"
    IBAN = "IBAN"
    IPV4 = "IPV4"
    IPV6 = "IPV6"
    PHONE = "PHONE"
    SECRET = "SECRET"
    US_SSN = "US_SSN"

    def __str__(self) -> str:
        return str(self.value)
