# provenance: original
"""Monty-subset lint for the stand-in (GUEST-02). Interface stub: no rules yet."""

import dataclasses


@dataclasses.dataclass(frozen=True, order=True)
class Violation:
    path: str
    line: int
    col: int
    rule: str
    message: str

    def __str__(self):
        return self.path + ":" + str(self.line) + ":" + str(self.col) + ": " + self.rule + " " + self.message


def check(source, path):
    return []
