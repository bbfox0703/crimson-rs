use pyo3::exceptions::{PyKeyError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

use crate::binary::{
    CArray, CFlaggedArray, COptional, CString, HeaderName, LocalizableString, OptLocalizableString,
};

// ── Traits ────────────────────────────────────────────────────────────────────

/// Convert a Rust value to a Python object.
pub trait ToPyValue {
    fn to_py_value(&self, py: Python<'_>) -> PyResult<Py<PyAny>>;
}

/// Read a Python value and write it as binary bytes.
pub trait WritePyValue {
    fn write_from_py(w: &mut Vec<u8>, obj: &Bound<'_, PyAny>) -> PyResult<()>;
}

// ── Dict helper (used by generated write_from_py_dict) ────────────────────────

pub fn get_field<'py>(d: &Bound<'py, PyDict>, key: &str) -> PyResult<Bound<'py, PyAny>> {
    d.get_item(key)?
        .ok_or_else(|| PyKeyError::new_err(key.to_string()))
}

// ── Primitives ────────────────────────────────────────────────────────────────

macro_rules! impl_primitive {
    ($t:ty) => {
        impl ToPyValue for $t {
            fn to_py_value(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
                Ok((*self).into_pyobject(py)?.into_any().unbind())
            }
        }

        impl WritePyValue for $t {
            fn write_from_py(w: &mut Vec<u8>, obj: &Bound<'_, PyAny>) -> PyResult<()> {
                let v: $t = obj.extract()?;
                w.extend_from_slice(&v.to_le_bytes());
                Ok(())
            }
        }
    };
}

impl_primitive!(u16);
impl_primitive!(u32);
impl_primitive!(u64);
impl_primitive!(i8);
impl_primitive!(i64);
impl_primitive!(f32);

// u8 is special: to_le_bytes returns [u8; 1], just push directly
impl ToPyValue for u8 {
    fn to_py_value(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        Ok((*self).into_pyobject(py)?.into_any().unbind())
    }
}

impl WritePyValue for u8 {
    fn write_from_py(w: &mut Vec<u8>, obj: &Bound<'_, PyAny>) -> PyResult<()> {
        let v: u8 = obj.extract()?;
        w.push(v);
        Ok(())
    }
}

// ── Fixed-size arrays ─────────────────────────────────────────────────────────

impl ToPyValue for [f32; 3] {
    fn to_py_value(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        Ok(self.to_vec().into_pyobject(py)?.into_any().unbind())
    }
}

impl WritePyValue for [f32; 3] {
    fn write_from_py(w: &mut Vec<u8>, obj: &Bound<'_, PyAny>) -> PyResult<()> {
        let list = obj.cast::<PyList>()?;
        // A list of the wrong length would write a short or long row that
        // fails only on the next parse, so it is refused here.
        if list.len() != 3 {
            return Err(PyValueError::new_err(format!("expected 3 values, got {}", list.len())));
        }
        for item in list.iter() {
            let v: f32 = item.extract()?;
            w.extend_from_slice(&v.to_le_bytes());
        }
        Ok(())
    }
}

impl ToPyValue for [u32; 2] {
    fn to_py_value(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        Ok(self.to_vec().into_pyobject(py)?.into_any().unbind())
    }
}

impl WritePyValue for [u32; 2] {
    fn write_from_py(w: &mut Vec<u8>, obj: &Bound<'_, PyAny>) -> PyResult<()> {
        let list = obj.cast::<PyList>()?;
        // A list of the wrong length would write a short or long row that
        // fails only on the next parse, so it is refused here.
        if list.len() != 2 {
            return Err(PyValueError::new_err(format!("expected 2 values, got {}", list.len())));
        }
        for item in list.iter() {
            let v: u32 = item.extract()?;
            w.extend_from_slice(&v.to_le_bytes());
        }
        Ok(())
    }
}

// ItemInfo `inventory_info_list` — twelve InventoryKey (u16) slots.
impl ToPyValue for [u16; 12] {
    fn to_py_value(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        Ok(self.to_vec().into_pyobject(py)?.into_any().unbind())
    }
}

impl WritePyValue for [u16; 12] {
    fn write_from_py(w: &mut Vec<u8>, obj: &Bound<'_, PyAny>) -> PyResult<()> {
        let list = obj.cast::<PyList>()?;
        // The slot count has changed across patches; a shorter list from an
        // export of an older patch would otherwise write an item short.
        if list.len() != 12 {
            return Err(PyValueError::new_err(format!(
                "inventory_info_list needs 12 InventoryKey slots, got {}",
                list.len()
            )));
        }
        for item in list.iter() {
            let v: u16 = item.extract()?;
            w.extend_from_slice(&v.to_le_bytes());
        }
        Ok(())
    }
}

impl ToPyValue for [u32; 4] {
    fn to_py_value(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        Ok(self.to_vec().into_pyobject(py)?.into_any().unbind())
    }
}

impl WritePyValue for [u32; 4] {
    fn write_from_py(w: &mut Vec<u8>, obj: &Bound<'_, PyAny>) -> PyResult<()> {
        let list = obj.cast::<PyList>()?;
        // A list of the wrong length would write a short or long row that
        // fails only on the next parse, so it is refused here.
        if list.len() != 4 {
            return Err(PyValueError::new_err(format!("expected 4 values, got {}", list.len())));
        }
        for item in list.iter() {
            let v: u32 = item.extract()?;
            w.extend_from_slice(&v.to_le_bytes());
        }
        Ok(())
    }
}


// ── CString ───────────────────────────────────────────────────────────────────

impl ToPyValue for CString<'_> {
    fn to_py_value(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        Ok(self.as_str().into_pyobject(py)?.into_any().unbind())
    }
}

impl WritePyValue for CString<'_> {
    fn write_from_py(w: &mut Vec<u8>, obj: &Bound<'_, PyAny>) -> PyResult<()> {
        let s: String = obj.extract()?;
        w.extend_from_slice(&(s.len() as u32).to_le_bytes());
        w.extend_from_slice(s.as_bytes());
        Ok(())
    }
}

// ── HeaderName ────────────────────────────────────────────────────────────────

impl ToPyValue for HeaderName<'_> {
    fn to_py_value(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        Ok(self.data.into_pyobject(py)?.into_any().unbind())
    }
}

impl WritePyValue for HeaderName<'_> {
    /// The name lives in the table's `.staticinfoheader`, so the row gets no bytes.
    fn write_from_py(_w: &mut Vec<u8>, _obj: &Bound<'_, PyAny>) -> PyResult<()> {
        Ok(())
    }
}

// ── LocalizableString ─────────────────────────────────────────────────────────

impl ToPyValue for LocalizableString<'_> {
    fn to_py_value(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let d = PyDict::new(py);
        d.set_item("category", self.category)?;
        d.set_item("index", self.index)?;
        d.set_item("default", self.default.as_str())?;
        Ok(d.into_any().unbind())
    }
}

impl WritePyValue for LocalizableString<'_> {
    fn write_from_py(w: &mut Vec<u8>, obj: &Bound<'_, PyAny>) -> PyResult<()> {
        let d = obj.cast::<PyDict>()?;
        let category: u8 = get_field(d, "category")?.extract()?;
        let index: u64 = get_field(d, "index")?.extract()?;
        let default: String = get_field(d, "default")?.extract()?;
        w.push(category);
        w.extend_from_slice(&index.to_le_bytes());
        w.extend_from_slice(&(default.len() as u32).to_le_bytes());
        w.extend_from_slice(default.as_bytes());
        Ok(())
    }
}

// ── OptLocalizableString ──────────────────────────────────────────────────────

impl ToPyValue for OptLocalizableString<'_> {
    fn to_py_value(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let d = PyDict::new(py);
        d.set_item("category", self.category)?;
        d.set_item("index", self.index)?;
        d.set_item("default", self.default.as_str())?;
        Ok(d.into_any().unbind())
    }
}

impl WritePyValue for OptLocalizableString<'_> {
    fn write_from_py(w: &mut Vec<u8>, obj: &Bound<'_, PyAny>) -> PyResult<()> {
        let d = obj.cast::<PyDict>()?;
        let category: u8 = get_field(d, "category")?.extract()?;
        w.push(category);
        if category == 0 {
            return Ok(());
        }
        let index: u64 = get_field(d, "index")?.extract()?;
        let default: String = get_field(d, "default")?.extract()?;
        w.extend_from_slice(&index.to_le_bytes());
        w.extend_from_slice(&(default.len() as u32).to_le_bytes());
        w.extend_from_slice(default.as_bytes());
        Ok(())
    }
}

// ── CArray ────────────────────────────────────────────────────────────────────

impl<T: ToPyValue> ToPyValue for CArray<T> {
    fn to_py_value(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let list = PyList::empty(py);
        for item in &self.items {
            list.append(item.to_py_value(py)?)?;
        }
        Ok(list.into_any().unbind())
    }
}

impl<T: WritePyValue> WritePyValue for CArray<T> {
    fn write_from_py(w: &mut Vec<u8>, obj: &Bound<'_, PyAny>) -> PyResult<()> {
        let list = obj.cast::<PyList>()?;
        w.extend_from_slice(&(list.len() as u32).to_le_bytes());
        for item in list.iter() {
            T::write_from_py(w, &item)?;
        }
        Ok(())
    }
}

// ── CFlaggedArray ─────────────────────────────────────────────────────────────

impl<T: ToPyValue> ToPyValue for CFlaggedArray<T> {
    fn to_py_value(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let list = PyList::empty(py);
        for item in &self.items {
            list.append(item.to_py_value(py)?)?;
        }
        Ok(list.into_any().unbind())
    }
}

impl<T: WritePyValue> WritePyValue for CFlaggedArray<T> {
    fn write_from_py(w: &mut Vec<u8>, obj: &Bound<'_, PyAny>) -> PyResult<()> {
        let list = obj.cast::<PyList>()?;
        if list.is_empty() {
            w.push(0);
            return Ok(());
        }
        w.push(1);
        w.extend_from_slice(&(list.len() as u32).to_le_bytes());
        for item in list.iter() {
            T::write_from_py(w, &item)?;
        }
        Ok(())
    }
}

// ── COptional ─────────────────────────────────────────────────────────────────

impl<T: ToPyValue> ToPyValue for COptional<T> {
    fn to_py_value(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        match &self.value {
            Some(v) => v.to_py_value(py),
            None => Ok(py.None()),
        }
    }
}

impl<T: WritePyValue> WritePyValue for COptional<T> {
    fn write_from_py(w: &mut Vec<u8>, obj: &Bound<'_, PyAny>) -> PyResult<()> {
        if obj.is_none() {
            w.push(0);
        } else {
            w.push(1);
            T::write_from_py(w, obj)?;
        }
        Ok(())
    }
}
