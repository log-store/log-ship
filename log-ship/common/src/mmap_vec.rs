use std::ops::Deref;
use std::sync::Arc;

use memmap::Mmap;

#[allow(unused_imports)]
use crate::{duration_us, logging::{debug}};

pub struct MMapVec<T> {
    mmap: Arc<Mmap>,
    vec: Vec<T>
}

// unsafe impl Send for MMapVec { }

impl <T: Sized> MMapVec<T> {
    /// Creates a new MMapVec, backed by the memory in the mmap
    /// `start` is the offset in *bytes* where the Vec should begin
    /// `length` is the length in *T* of the Vec
    pub fn new(mmap: Arc<Mmap>, start: usize, length: usize) -> Self {
        let vec = unsafe {
            let ptr = mmap.deref().as_ptr().offset(start as isize) as *mut u8 as *mut T;
            Vec::<T>::from_raw_parts(ptr, length, length)
        };

        MMapVec {
            mmap,
            vec
        }
    }
}

impl <T: Sized> AsRef<[T]> for MMapVec<T> {
    fn as_ref(&self) -> &[T] {
        self.vec.as_slice()
    }
}

impl <T: Sized> Drop for MMapVec<T> {
    fn drop(&mut self) {
        std::mem::forget(std::mem::take(&mut self.vec))
    }
}
