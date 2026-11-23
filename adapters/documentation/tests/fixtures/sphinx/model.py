"""Public record model, consumed by :func:`api.describe`."""


class Record:
    """One retained value.

    Example::

        record = Record(42, label="sample")
        assert record.value == 42
    """

    def __init__(self, value: int, *, label: str = "sample") -> None:
        self.value = value
        self.label = label

    def scaled(self, factor: int = 2) -> int:
        """Return the value multiplied by the selected factor."""
        return self.value * factor
