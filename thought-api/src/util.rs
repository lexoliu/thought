use std::{alloc::Layout, ptr::slice_from_raw_parts_mut};

#[no_mangle]
pub extern "C" fn thought_alloc(len: usize) -> *mut u8 {
    let layout = Layout::array::<u8>(len).unwrap();
    unsafe { std::alloc::alloc(layout) }
}

#[repr(C)]
pub struct Data {
    head: *mut u8,
    len: usize,
}

impl From<Data> for Box<[u8]> {
    fn from(val: Data) -> Self {
        unsafe { Box::from_raw(slice_from_raw_parts_mut(val.head, val.len)) }
    }
}

impl From<Data> for Vec<u8> {
    fn from(val: Data) -> Self {
        let boxed: Box<[u8]> = val.into();
        boxed.into_vec()
    }
}

impl Data {
    pub fn into_vec(self) -> Vec<u8> {
        self.into()
    }
}

extern "C" {
    pub fn thought_get_article(id: usize) -> Data;
    pub fn thought_get_article_content(id: usize) -> Data;
}
