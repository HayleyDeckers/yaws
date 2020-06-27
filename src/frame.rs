//make a frame a singular entity in ram? requires a deep copy of message before sending data. Could use an api where Frame is _like_ a vector. Having a capacity and size.
// OR: give frame reference to the data, keeping the header part seperate? (could use both types)

// some messages can be reused, here a reference type would win out in performance, unless we don't consume frames.
// using in place frames saves on programming complexity however. Allows move-everywhere when we don't have to reuse messages.

//def look up what the interface is for async io in rust.
//
// by using fixed size frames (DST type) we could build a very memory optimized framework however. With well defined limits on its size and 'no' dynamic allocation.
// well suited for IOT devices.

//ideally: we build our frame based on Async writers (flexible, no double copy or masking problems)
// takes an AsyncWriter ( futures::io::AsyncWrite )as input and a byte limit.
// We then allocate a buffer for it (pref from one of several reusable arenas to allow for multithreading and such)
// if the size is not predetermined/predeterminable such as would be the case for a streaming source.
// we should allocate an initial buffer large enough to store at least the the header + masking key + max size.
// after the stream terminates, or is otherwise flushed (due to timeout? or size limit being reached, we fill out the header)
// as the header size is dynamic and needs to be the minimum size according to the spec, we cannot predetermine this without first caching
// the whole output of the source.
//
// so we take a AsyncWriter, read up-to-N bytes from it into a buffer and mask it with the key.
// then we vector I/O write the header stuff followed by the data. If there's still data left in the source, we repeat
//
// An optimization might be to not use vector IO but a single preallocated pointer. or a special vector struct. a list of smaller sized blocks?
//
// impl ASyncWriteOverWebsocket for all T : ASyncWrite, or well basically: api should be transparant. Make a websocket interface that accepts anything that's AsyncWrite.

// the websocket server should be ASyncWrite.
// anything written to it gets wrapped into a frame, then send over the physical socket
//
// if multiple large messages need to be send to one client (such as a shiny update, multiple elements firing at once,
// we should wrap this in some way that makes sure that multiple can writes can be outgoing ot the same socket at once)
// perhaps an extension, that simply wraps each message with an id and count. the receiving end should then dissassemble this.

// extern crate hayley_dststore;
// use hayley_dststore::*;

// a good interface would be a binary or text based async stream.
//
//

//there's a no-std vrsion of this.
use std::alloc::{alloc, Layout};
use std::fmt::Debug;
pub struct Frame {
    fin_rsv_opcode: u8,
    mask_len: u8,
    dynamic: [u8],
}

impl Frame {
    fn create_layout(dynamic_size: usize) -> Layout {
        Layout::array::<u8>(2 + dynamic_size).unwrap()
    }
    //how do we put this on the stack...?
    unsafe fn new_boxed_uninit(dynamic_size: usize) -> Box<Self> {
        let layout = Self::create_layout(dynamic_size);
        let boxed_ptr = {
            let raw_ptr = alloc(layout);
            if raw_ptr == std::ptr::null_mut() {
                std::alloc::handle_alloc_error(layout);
            }
            let raw_slice = std::slice::from_raw_parts_mut(raw_ptr, 2 + dynamic_size);
            Box::from_raw(std::mem::transmute::<&mut [u8], *mut Self>(raw_slice))
        };
        boxed_ptr
    }
    pub unsafe fn from_slice_unchecked(slice: &[u8]) -> &Frame {
        std::mem::transmute::<&[u8], &Frame>(slice)
    }
    pub fn fin(&self) -> bool {
        (self.fin_rsv_opcode & 128) != 0
    }
    pub fn rsv1(&self) -> bool {
        (self.fin_rsv_opcode & (1 << 6)) != 0
    }
    pub fn rsv2(&self) -> bool {
        (self.fin_rsv_opcode & (1 << 5)) != 0
    }
    pub fn rsv3(&self) -> bool {
        (self.fin_rsv_opcode & (1 << 4)) != 0
    }
    pub fn opcode(&self) -> u8 {
        self.fin_rsv_opcode & 0xf
    }
    pub fn mask(&self) -> Option<u32> {
        if self.has_mask() {
            let len = self.mask_len % 128;
            let offset = if len == 127 {
                8
            } else if len == 126 {
                2
            } else {
                0
            };
            let bytes = [
                self.dynamic[offset + 0],
                self.dynamic[offset + 1],
                self.dynamic[offset + 2],
                self.dynamic[offset + 3],
            ];
            Some(u32::from_be_bytes(bytes))
        } else {
            None
        }
    }
    pub fn is_close(&self) -> bool {
        self.opcode() == 0x8
    }
    pub fn is_ping(&self) -> bool {
        self.opcode() == 0x9
    }
    pub fn is_bin(&self) -> bool {
        self.opcode() == 0x2
    }
    pub fn is_text(&self) -> bool {
        self.opcode() == 0x1
    }
    pub fn has_mask(&self) -> bool {
        self.mask_len >= 1 << 7
    }
    pub fn masked_data(&self) -> &[u8] {
        let len = self.mask_len % 128;
        let offset = if self.has_mask() { 4 } else { 0 }
            + if len == 127 {
                8
            } else if len == 126 {
                2
            } else {
                0
            };
        &self.dynamic[offset..offset + self.data_len()]
    }
    pub fn is_final(&self) -> bool {
        self.fin_rsv_opcode >= 1 << 7
    }
    pub fn data_len(&self) -> usize {
        let len = self.mask_len % 128;
        if len == 127 {
            let bytes = [
                self.dynamic[0],
                self.dynamic[1],
                self.dynamic[2],
                self.dynamic[3],
                self.dynamic[4],
                self.dynamic[5],
                self.dynamic[6],
                self.dynamic[7],
            ];
            u64::from_be_bytes(bytes) as usize
        } else if len == 126 {
            let bytes = [self.dynamic[0], self.dynamic[1]];
            u16::from_be_bytes(bytes) as usize
        } else {
            len as usize
        }
    }
    fn new_raw(buf: &[u8], mask: Option<u32>, opcode: u8) -> Box<Frame> {
        let fin_rsv_opcode = (1 << 7) | opcode;
        let payload_length: u8 = if buf.len() <= 125 {
            buf.len() as u8
        } else if buf.len() <= (1 << 16) - 1 {
            126
        } else {
            127
        };
        let mask_len = if mask.is_some() {
            (1 << 7) | payload_length
        } else {
            payload_length
        };
        let dynamic_size = buf.len()
            + if mask.is_some() { 4 } else { 0 }
            + if payload_length == 126 {
                2
            } else if payload_length == 127 {
                8
            } else {
                0
            };
        //TODO: can  probably do all of this with iterator chaining.
        let mut this = unsafe { Self::new_boxed_uninit(dynamic_size) };
        this.fin_rsv_opcode = fin_rsv_opcode;
        this.mask_len = mask_len;
        let mut payload_idx = 0;
        if payload_length == 126 {
            let size = (buf.len() as u16).to_be_bytes();
            for i in 0..size.len() {
                this.dynamic[payload_idx + i] = size[i];
            }
            payload_idx += 2;
        } else if payload_length == 127 {
            if payload_length == 126 {
                let size = (buf.len() as u64).to_be_bytes();
                for i in 0..size.len() {
                    this.dynamic[payload_idx + i] = size[i];
                }
                payload_idx += 8;
            }
        }
        let mask_array = mask.unwrap_or(0).to_be_bytes();
        if mask.is_some() {
            for i in 0..mask_array.len() {
                this.dynamic[payload_idx + i] = mask_array[i];
            }
            payload_idx += 4;
        }
        for i in 0..buf.len() {
            this.dynamic[payload_idx + i] = buf[i] ^ mask_array[i % 4];
        }
        this
    }
    pub fn new_close(code: Option<u16>, mask: Option<u32>) -> Box<Frame> {
        Self::new_raw(&code.unwrap_or(1000).to_be_bytes(), mask, 0x8)
    }
    pub fn new_binary(buf: &[u8], mask: Option<u32>) -> Box<Frame> {
        Self::new_raw(buf, mask, 0x2)
    }
    pub fn new_ping(buf: Option<&[u8]>, mask: Option<u32>) -> Box<Frame> {
        Self::new_raw(buf.unwrap_or(&[]), mask, 0x9)
    }
    pub fn new_text<S: AsRef<str>>(buf: S, mask: Option<u32>) -> Box<Frame> {
        Self::new_raw(buf.as_ref().as_bytes(), mask, 0x1)
    }
    pub fn as_bytes(&self) -> &[u8] {
        unsafe {
            let len = self.mask_len % 128;
            let offset = if self.has_mask() { 4 } else { 0 }
                + if len == 127 {
                    8
                } else if len == 126 {
                    2
                } else {
                    0
                };
            std::slice::from_raw_parts(
                (self as *const Self) as *const u8,
                2 + offset + self.data_len(),
            )
        }
    }
}

impl Debug for Frame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Frame")
            .field("fin", &self.fin())
            .field("rsv1", &self.rsv1())
            .field("rsv2", &self.rsv2())
            .field("rsv3", &self.rsv3())
            .field("opcode", &self.opcode())
            .field("mask", &self.has_mask())
            .field("payload_len", &(self.mask_len & 127))
            .field("real payload_len", &self.data_len())
            .field("mask", &self.mask())
            .field("data", &self.masked_data())
            .finish()
    }
}
// extern crate futures;
// use futures::io::{AsyncRead, AsyncWrite};

pub struct FrameBuilder {
    n_bytes: usize,
    mask: u16,
    buffer: Vec<u8>,
}

// impl FrameBuilder {
//     pub fn from_slice(buf: &[u8]) -> FrameBuilder {}
// }

// pub fn framed<T: AsyncWrite>(target: T) -> FrameBuilder<T> {
//     FrameBuilder {
//         n_bytes: 0,
//         mask: 5, // chosen at random by dice-roll
//         target,
//         buffer: Vec::new(),
//     }
// }

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
