use std::io::{self, Write};

use super::{BinaryRead, BinaryReadTracked, BinaryWrite, FieldRange, pop_path, push_index};

impl<'a> BinaryRead<'a> for [f32; 3] {
    fn read_from(data: &'a [u8], offset: &mut usize) -> io::Result<Self> {
        Ok([
            f32::read_from(data, offset)?,
            f32::read_from(data, offset)?,
            f32::read_from(data, offset)?,
        ])
    }
}

impl BinaryWrite for [f32; 3] {
    fn write_to(&self, w: &mut dyn Write) -> io::Result<()> {
        for v in self {
            v.write_to(w)?;
        }
        Ok(())
    }
}

impl<'a> BinaryRead<'a> for [u32; 2] {
    fn read_from(data: &'a [u8], offset: &mut usize) -> io::Result<Self> {
        Ok([u32::read_from(data, offset)?, u32::read_from(data, offset)?])
    }
}

impl BinaryWrite for [u32; 2] {
    fn write_to(&self, w: &mut dyn Write) -> io::Result<()> {
        for v in self {
            v.write_to(w)?;
        }
        Ok(())
    }
}

impl<'a> BinaryRead<'a> for [u32; 4] {
    fn read_from(data: &'a [u8], offset: &mut usize) -> io::Result<Self> {
        Ok([
            u32::read_from(data, offset)?,
            u32::read_from(data, offset)?,
            u32::read_from(data, offset)?,
            u32::read_from(data, offset)?,
        ])
    }
}

impl BinaryWrite for [u32; 4] {
    fn write_to(&self, w: &mut dyn Write) -> io::Result<()> {
        for v in self {
            v.write_to(w)?;
        }
        Ok(())
    }
}

impl<'a> BinaryRead<'a> for [u8; 3] {
    fn read_from(data: &'a [u8], offset: &mut usize) -> io::Result<Self> {
        super::check_remaining(data, *offset, 3)?;
        let arr = [data[*offset], data[*offset + 1], data[*offset + 2]];
        *offset += 3;
        Ok(arr)
    }
}

impl BinaryWrite for [u8; 3] {
    fn write_to(&self, w: &mut dyn Write) -> io::Result<()> {
        w.write_all(self)
    }
}

impl<'a> BinaryRead<'a> for [u8; 22] {
    fn read_from(data: &'a [u8], offset: &mut usize) -> io::Result<Self> {
        super::check_remaining(data, *offset, 22)?;
        let mut arr = [0u8; 22];
        arr.copy_from_slice(&data[*offset..*offset + 22]);
        *offset += 22;
        Ok(arr)
    }
}

impl BinaryWrite for [u8; 22] {
    fn write_to(&self, w: &mut dyn Write) -> io::Result<()> {
        w.write_all(self)
    }
}

impl<'a> BinaryReadTracked<'a> for [u8; 22] {
    fn read_tracked(
        data: &'a [u8],
        offset: &mut usize,
        path: &mut String,
        ranges: &mut Vec<FieldRange>,
    ) -> io::Result<Self> {
        let start = *offset;
        let arr = <[u8; 22] as BinaryRead>::read_from(data, offset)?;
        ranges.push(FieldRange {
            path: path.clone(),
            start,
            end: *offset,
            ty: "u8x22",
        });
        Ok(arr)
    }
}

// ── 9-byte raw block (Crimson Desert 1.05 ItemInfo trailing pad) ────────────

impl<'a> BinaryRead<'a> for [u8; 9] {
    fn read_from(data: &'a [u8], offset: &mut usize) -> io::Result<Self> {
        super::check_remaining(data, *offset, 9)?;
        let mut arr = [0u8; 9];
        arr.copy_from_slice(&data[*offset..*offset + 9]);
        *offset += 9;
        Ok(arr)
    }
}

impl BinaryWrite for [u8; 9] {
    fn write_to(&self, w: &mut dyn Write) -> io::Result<()> {
        w.write_all(self)
    }
}

impl<'a> BinaryReadTracked<'a> for [u8; 9] {
    fn read_tracked(
        data: &'a [u8],
        offset: &mut usize,
        path: &mut String,
        ranges: &mut Vec<FieldRange>,
    ) -> io::Result<Self> {
        let start = *offset;
        let arr = <[u8; 9] as BinaryRead>::read_from(data, offset)?;
        ranges.push(FieldRange {
            path: path.clone(),
            start,
            end: *offset,
            ty: "u8x9",
        });
        Ok(arr)
    }
}

// ── Fixed-size array tracked reads ──────────────────────────────────────────
// Each element is reported as `<path>[i]` so the byte layout is preserved.

impl<'a> BinaryReadTracked<'a> for [f32; 3] {
    fn read_tracked(
        data: &'a [u8],
        offset: &mut usize,
        path: &mut String,
        ranges: &mut Vec<FieldRange>,
    ) -> io::Result<Self> {
        let mut out = [0f32; 3];
        for (i, slot) in out.iter_mut().enumerate() {
            let saved = push_index(path, i);
            *slot = f32::read_tracked(data, offset, path, ranges)?;
            pop_path(path, saved);
        }
        Ok(out)
    }
}

impl<'a> BinaryReadTracked<'a> for [u32; 4] {
    fn read_tracked(
        data: &'a [u8],
        offset: &mut usize,
        path: &mut String,
        ranges: &mut Vec<FieldRange>,
    ) -> io::Result<Self> {
        let mut out = [0u32; 4];
        for (i, slot) in out.iter_mut().enumerate() {
            let saved = push_index(path, i);
            *slot = u32::read_tracked(data, offset, path, ranges)?;
            pop_path(path, saved);
        }
        Ok(out)
    }
}

impl<'a> BinaryReadTracked<'a> for [u32; 2] {
    fn read_tracked(
        data: &'a [u8],
        offset: &mut usize,
        path: &mut String,
        ranges: &mut Vec<FieldRange>,
    ) -> io::Result<Self> {
        let mut out = [0u32; 2];
        for (i, slot) in out.iter_mut().enumerate() {
            let saved = push_index(path, i);
            *slot = u32::read_tracked(data, offset, path, ranges)?;
            pop_path(path, saved);
        }
        Ok(out)
    }
}

impl<'a> BinaryReadTracked<'a> for [u8; 3] {
    fn read_tracked(
        data: &'a [u8],
        offset: &mut usize,
        path: &mut String,
        ranges: &mut Vec<FieldRange>,
    ) -> io::Result<Self> {
        let mut out = [0u8; 3];
        for (i, slot) in out.iter_mut().enumerate() {
            let saved = push_index(path, i);
            *slot = u8::read_tracked(data, offset, path, ranges)?;
            pop_path(path, saved);
        }
        Ok(out)
    }
}
