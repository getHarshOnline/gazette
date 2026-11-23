"""Operations over the public :class:`model.Record` model."""

from model import Record


def describe(record: Record, /, *, prefix: str = "value") -> str:
    """Describe a :class:`model.Record` without changing it.

    Example::

        describe(Record(42)) == "value: 42"
    """
    return f"{prefix}: {record.value}"
