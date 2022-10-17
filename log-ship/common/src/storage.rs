use std::io::{Read, Seek, SeekFrom, Write};
use std::mem::{size_of, size_of_val};
use std::{ptr, slice};

/// Type for RowId
pub type RowId = u32;

pub trait StructToBytes {
    /// Converts the structure to a byte slice
    fn to_bytes(&self) -> &[u8] {
        unsafe {
            // NOTE: This function does not have a Self: Sized bound.
            // size_of_val works for unsized values too.
            let len = size_of_val(self);
            // debug!("to_bytes: {}", len);
            slice::from_raw_parts(self as *const Self as *const u8, len)
        }
    }

    /// Writes and flushes the structure to disk
    fn write_struct<W: Write + Seek>(&self, writer: &mut W, loc: SeekFrom) -> Result<(), std::io::Error> {
        writer.seek(loc)?;
        writer.write_all(self.to_bytes())?;
        writer.flush()
    }

    /// Reads a struct from a reader
    fn read_struct<R: Read>(reader: &mut R) -> Result<Self, std::io::Error> where Self: Sized {
        let len = size_of::<Self>();
        let mut buff = vec![0u8; len];

        // debug!("Header len: {}", len);

        reader.read_exact(buff.as_mut_slice())?;

        Ok( unsafe { ptr::read(buff.as_ptr() as *const Self) })
    }
}

