mod arrays;
pub(crate) mod paloc;
pub(crate) mod pamt;
pub(crate) mod papgt;
pub(crate) mod paver;
pub(crate) mod paz;
mod primitives;
pub(crate) mod trie;
mod types;

pub use types::*;

use std::io::{self, Write};

// ── Traits ──────────────────────────────────────────────────────────────────

pub trait BinaryRead<'a>: Sized {
    fn read_from(data: &'a [u8], offset: &mut usize) -> io::Result<Self>;
}

pub trait BinaryWrite {
    fn write_to(&self, writer: &mut dyn Write) -> io::Result<()>;
}

// ── Range tracking (used to map file bytes → field paths) ───────────────────
//
// Parallel to `BinaryRead`. `read_tracked` walks the same bytes in the same
// order as `read_from`, but also records a `FieldRange` for every leaf
// consumed — so callers can answer "what field does byte N of entry X
// belong to?" with a binary-search lookup.
//
// `path` is a mutable buffer reused across recursion to avoid per-call
// allocation: children push a segment before recursing, then truncate back
// to the parent's length.

#[derive(Debug, Clone)]
pub struct FieldRange {
    pub path: String,
    pub start: usize,
    pub end: usize,
    pub ty: &'static str,
}

pub trait BinaryReadTracked<'a>: Sized {
    fn read_tracked(
        data: &'a [u8],
        offset: &mut usize,
        path: &mut String,
        ranges: &mut Vec<FieldRange>,
    ) -> io::Result<Self>;
}

/// Push a child segment onto `path`, returning the previous length so
/// the caller can restore it. Uses `.` separator except at the root.
#[inline]
pub(crate) fn push_path(path: &mut String, seg: &str) -> usize {
    let saved = path.len();
    if !path.is_empty() {
        path.push('.');
    }
    path.push_str(seg);
    saved
}

/// Push an array index `[i]` onto `path`.
#[inline]
pub(crate) fn push_index(path: &mut String, i: usize) -> usize {
    let saved = path.len();
    use std::fmt::Write as _;
    write!(path, "[{}]", i).expect("fmt to String");
    saved
}

#[inline]
pub(crate) fn pop_path(path: &mut String, saved: usize) {
    path.truncate(saved);
}

// ── Helpers ─────────────────────────────────────────────────────────────────

pub(crate) fn check_remaining(data: &[u8], offset: usize, need: usize) -> io::Result<()> {
    if offset + need > data.len() {
        Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "not enough data",
        ))
    } else {
        Ok(())
    }
}

// ── Macro for simple structs (binary only, no Python conversion) ────────────

#[macro_export]
macro_rules! binary_struct {
    (
        $(#[$meta:meta])*
        pub struct $name:ident $(<$lt:lifetime>)? {
            $(pub $field:ident : $ty:ty),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug)]
        pub struct $name $(<$lt>)? {
            $(pub $field: $ty),*
        }

        impl<'a> $crate::binary::BinaryRead<'a> for $name $(<$lt>)? {
            fn read_from(data: &'a [u8], offset: &mut usize) -> std::io::Result<Self> {
                Ok($name {
                    $($field: $crate::binary::BinaryRead::read_from(data, offset)?),*
                })
            }
        }

        impl $(< $lt >)? $crate::binary::BinaryWrite for $name $(< $lt >)? {
            fn write_to(&self, w: &mut dyn std::io::Write) -> std::io::Result<()> {
                $($crate::binary::BinaryWrite::write_to(&self.$field, w)?;)*
                Ok(())
            }
        }
    };
}

// ── Macro for structs with binary + Python conversion ───────────────────────

// ── Conditional-field helpers for py_binary_struct ──────────────────────────
//
// A field may carry an optional `=> <cond>` clause, meaning it is only present
// on disk when `<cond>` (an expression over the *earlier* fields of the same
// struct) is true. Used for patch drifts where Pearl Abyss gates a field on a
// sibling value — e.g. Crimson Desert 1.12's `unk_pre_gimmick_visual`, present
// only for equipment / gem items. The condition is evaluated **only in the read
// path** (where the prior fields are in scope as locals); a conditional field is
// stored as `Option<T>` so write / to_py / from_py drive off `Option` presence
// and never need to re-evaluate the condition (which keeps the write path from
// having to reference siblings through `self`).

#[doc(hidden)]
#[macro_export]
macro_rules! __pbs_field_ty {
    ($ty:ty) => { $ty };
    ($ty:ty, $cond:expr) => { ::core::option::Option<$ty> };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __pbs_read {
    ($data:expr, $offset:expr, $ty:ty) => {
        <$ty as $crate::binary::BinaryRead>::read_from($data, $offset)?
    };
    ($data:expr, $offset:expr, $ty:ty, $cond:expr) => {
        if $cond {
            ::core::option::Option::Some(
                <$ty as $crate::binary::BinaryRead>::read_from($data, $offset)?,
            )
        } else {
            ::core::option::Option::None
        }
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __pbs_read_tracked {
    ($data:expr, $offset:expr, $path:expr, $ranges:expr, $field:ident, $ty:ty) => {{
        let __saved = $crate::binary::push_path($path, stringify!($field));
        let __v = <$ty as $crate::binary::BinaryReadTracked>::read_tracked(
            $data, $offset, $path, $ranges,
        )?;
        $crate::binary::pop_path($path, __saved);
        __v
    }};
    ($data:expr, $offset:expr, $path:expr, $ranges:expr, $field:ident, $ty:ty, $cond:expr) => {{
        if $cond {
            let __saved = $crate::binary::push_path($path, stringify!($field));
            let __v = <$ty as $crate::binary::BinaryReadTracked>::read_tracked(
                $data, $offset, $path, $ranges,
            )?;
            $crate::binary::pop_path($path, __saved);
            ::core::option::Option::Some(__v)
        } else {
            ::core::option::Option::None
        }
    }};
}

#[doc(hidden)]
#[macro_export]
macro_rules! __pbs_write {
    ($self:ident, $w:expr, $field:ident) => {
        $crate::binary::BinaryWrite::write_to(&$self.$field, $w)?;
    };
    ($self:ident, $w:expr, $field:ident, $cond:expr) => {
        if let ::core::option::Option::Some(__v) = &$self.$field {
            $crate::binary::BinaryWrite::write_to(__v, $w)?;
        }
    };
}

#[doc(hidden)]
#[cfg(feature = "python")]
#[macro_export]
macro_rules! __pbs_to_py {
    ($py:ident, $self:ident, $field:ident) => {
        $crate::python_traits::ToPyValue::to_py_value(&$self.$field, $py)?
    };
    ($py:ident, $self:ident, $field:ident, $cond:expr) => {
        match &$self.$field {
            ::core::option::Option::Some(__v) =>
                $crate::python_traits::ToPyValue::to_py_value(__v, $py)?,
            ::core::option::Option::None => $py.None(),
        }
    };
}

#[doc(hidden)]
#[cfg(feature = "python")]
#[macro_export]
macro_rules! __pbs_write_py {
    ($w:expr, $d:expr, $field:ident, $ty:ty) => {
        <$ty as $crate::python_traits::WritePyValue>::write_from_py(
            $w, &$crate::python_traits::get_field($d, stringify!($field))?,
        )?;
    };
    ($w:expr, $d:expr, $field:ident, $ty:ty, $cond:expr) => {{
        let __f = $crate::python_traits::get_field($d, stringify!($field))?;
        if !pyo3::types::PyAnyMethods::is_none(&__f) {
            <$ty as $crate::python_traits::WritePyValue>::write_from_py($w, &__f)?;
        }
    }};
}

#[macro_export]
macro_rules! py_binary_struct {
    (
        $(#[$meta:meta])*
        pub struct $name:ident $(<$lt:lifetime>)? {
            $(pub $field:ident : $ty:ty $(=> $cond:expr)?),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug)]
        pub struct $name $(<$lt>)? {
            $(pub $field: $crate::__pbs_field_ty!($ty $(, $cond)?)),*
        }

        impl<'a> $crate::binary::BinaryRead<'a> for $name $(<$lt>)? {
            fn read_from(data: &'a [u8], offset: &mut usize) -> std::io::Result<Self> {
                $(
                    let $field = $crate::__pbs_read!(data, offset, $ty $(, $cond)?);
                )*
                Ok($name { $($field),* })
            }
        }

        impl<'a> $crate::binary::BinaryReadTracked<'a> for $name $(<$lt>)? {
            fn read_tracked(
                data: &'a [u8],
                offset: &mut usize,
                path: &mut String,
                ranges: &mut Vec<$crate::binary::FieldRange>,
            ) -> std::io::Result<Self> {
                $(
                    let $field = $crate::__pbs_read_tracked!(
                        data, offset, path, ranges, $field, $ty $(, $cond)?
                    );
                )*
                Ok($name { $($field),* })
            }
        }

        impl $(< $lt >)? $crate::binary::BinaryWrite for $name $(< $lt >)? {
            fn write_to(&self, w: &mut dyn std::io::Write) -> std::io::Result<()> {
                $( $crate::__pbs_write!(self, w, $field $(, $cond)?); )*
                Ok(())
            }
        }

        #[cfg(feature = "python")]
        impl $(< $lt >)? $name $(< $lt >)? {
            pub fn to_py_dict<'py>(&self, py: pyo3::Python<'py>)
                -> pyo3::PyResult<pyo3::Bound<'py, pyo3::types::PyDict>>
            {
                use pyo3::types::PyDictMethods;
                let d = pyo3::types::PyDict::new(py);
                $(
                    d.set_item(
                        stringify!($field),
                        $crate::__pbs_to_py!(py, self, $field $(, $cond)?),
                    )?;
                )*
                Ok(d)
            }

            pub fn write_from_py_dict(
                w: &mut Vec<u8>,
                d: &pyo3::Bound<'_, pyo3::types::PyDict>,
            ) -> pyo3::PyResult<()> {
                $( $crate::__pbs_write_py!(w, d, $field, $ty $(, $cond)?); )*
                Ok(())
            }
        }

        #[cfg(feature = "python")]
        impl $(< $lt >)? $crate::python_traits::ToPyValue for $name $(< $lt >)? {
            fn to_py_value(&self, py: pyo3::Python<'_>) -> pyo3::PyResult<pyo3::Py<pyo3::PyAny>> {
                Ok(self.to_py_dict(py)?.into_any().unbind())
            }
        }

        #[cfg(feature = "python")]
        impl $(< $lt >)? $crate::python_traits::WritePyValue for $name $(< $lt >)? {
            fn write_from_py(w: &mut Vec<u8>, obj: &pyo3::Bound<'_, pyo3::PyAny>) -> pyo3::PyResult<()> {
                Self::write_from_py_dict(w, obj.cast::<pyo3::types::PyDict>()?)
            }
        }
    };
}
