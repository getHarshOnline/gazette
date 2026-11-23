//! Provider qualification fixture, not an HTTP API description.
//! The [`model::Record`] is used by [`describe`].

/// Domain model module.
pub mod model {
    /// A record carrying its original value.
    ///
    /// ```
    /// use reference_fixture::model::Record;
    /// let record = Record { value: 3_u32 };
    /// assert_eq!(record.value, 3);
    /// ```
    pub struct Record<T> {
        /// Original payload, without conversion.
        pub value: T,
    }

    impl<T> Record<T> {
        /// Stable category retained as an associated item.
        pub const CATEGORY: &'static str = "record";

        /// Borrows the payload without changing its type-level meaning.
        ///
        /// ```
        /// use reference_fixture::model::Record;
        /// let record = Record { value: 3_u32 };
        /// assert_eq!(*record.as_ref().value, 3);
        /// ```
        pub fn as_ref(&self) -> Record<&T> {
            Record { value: &self.value }
        }
    }

    /// Whether publication was selected.
    pub enum Selection {
        /// Explicitly selected.
        Selected,
        /// Explicitly disabled with a reason.
        Disabled(String),
    }
}

/// A second module proves nested navigation and cross-module symbol links.
pub mod view {
    use crate::model::Record;

    /// A borrowed summary of a [`Record`].
    ///
    /// External project reference remains an explicit link to
    /// [`serde::Serialize`](https://docs.rs/serde/latest/serde/trait.Serialize.html).
    pub struct Summary<'a, T: ?Sized> {
        /// Caller-owned display label.
        pub label: &'a str,
        /// Borrowed original value.
        pub value: &'a T,
    }

    /// Builds a [`Summary`] from a record in the sibling module.
    pub fn summarize<T>(record: &Record<T>) -> Summary<'_, T> {
        Summary {
            label: "record",
            value: &record.value,
        }
    }
}

/// Describes a [`model::Record`] without taking ownership.
pub fn describe<T: core::fmt::Display>(record: &model::Record<T>) -> String {
    record.value.to_string()
}

/// A second public name must not disappear during conversion.
pub use model::Record as PublicRecord;

/// Trait and associated method signatures belong to the reference.
pub trait Describe {
    /// Result type retained by reference tooling.
    type Output;

    /// Borrowed description.
    fn description(&self) -> Self::Output;
}

/// Default upper bound; see [`DEFAULT_LABEL`].
pub const MAX_RECORDS: usize = 8;

/// Label for at most [`MAX_RECORDS`] retained records.
pub static DEFAULT_LABEL: &str = "reference";

/// Constructs a [`model::Record`] without converting its value.
///
/// ```
/// let record = reference_fixture::record!(7_u32);
/// assert_eq!(record.value, 7);
/// ```
///
/// See also [the same macro](macro.record!.html).
#[macro_export]
macro_rules! record {
    ($value:expr) => {
        $crate::model::Record { value: $value }
    };
}

/// Alternative representations of the same word, not two independent values.
///
/// The caller must know which field is active before reading it.
#[repr(C)]
pub union Word {
    /// The word interpreted as an unsigned integer.
    pub integer: u32,
    /// The word interpreted as a floating-point value.
    pub floating: f32,
}

/// Describes a record using the shared [`describe`] function.
impl<T: core::fmt::Display> Describe for model::Record<T> {
    type Output = String;

    /// Returns the retained payload description through [`describe`].
    fn description(&self) -> Self::Output {
        describe(self)
    }
}
