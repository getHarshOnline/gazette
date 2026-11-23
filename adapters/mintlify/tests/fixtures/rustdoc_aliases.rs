//! Native alias reference.
//! [Alias method](type.First.html#method.retained).
/// A stored record.
pub struct Record<T> {
    /// Original value.
    pub value: T,
}
/// Describes a stored record.
pub trait Describe {
    /// Return the record description.
    fn describe(&self) -> &'static str;
}
impl<T> Describe for Record<T> {
    fn describe(&self) -> &'static str {
        "stored record"
    }
}
impl<T> Record<T> {
    /// Borrow the original value.
    ///
    /// # Examples
    /// ```
    /// let record = reference_fixture::Record { value: 1_u8 };
    /// assert_eq!(*record.retained(), 1);
    /// ```
    pub fn retained(&self) -> &T {
        &self.value
    }
}
/// A byte record.
pub type First = Record<u8>;
/// A wider record.
pub type Second = Record<u16>;
