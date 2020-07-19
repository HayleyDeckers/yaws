use std::alloc::{alloc, Layout};
use std::fmt::Debug;
pub struct Frame {
    fin_rsv_opcode: u8,
    mask_len: u8,
    dynamic: [u8],
}
#[derive(Debug)]
pub enum Opcode {
    Binary,
    Text,
    Ping,
    Pong,
    Close,
    Invalid(u8),
}

pub enum ContentLength {
    EightBytes,
    TwoBytes,
    OneByte(u8),
}

// casting slice to frame can fail in several ways:
// * wrongly formatted header/not a frame
// * header to small, minimum 2 bytes,  if 2 bytes could need more bytes to determine message size.
// * if header is correct, then is the dynamic bit large enough?
//      if not we could return an incomplete data segment.
// * if it does fit, return a complete WS, and the remainder of the buffer.
#[derive(Debug, Clone, Copy)]
pub enum WsParsingError {
    // NotAFrame,
    IncompleteHeader,
    IncompleteMessage(usize),
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
    //note, this takes the whole slice, even if the message is only a small part.
    pub unsafe fn from_slice_unchecked(slice: &[u8]) -> &Frame {
        std::mem::transmute::<&[u8], &Frame>(slice)
    }
    pub fn len(&self) -> usize {
        self.header_size() + self.data_len()
    }
    pub fn parse_slice(slice: &[u8]) -> Result<(&Frame, &[u8]), WsParsingError> {
        //the minimum WS frame size is 2.
        if slice.len() < 2 {
            Err(WsParsingError::IncompleteHeader)
        } else {
            let frame_size = unsafe {
                let frame = Self::from_slice_unchecked(slice);
                //check if it's safe to read the size of the data segment
                if slice.len() < frame.header_size() {
                    return Err(WsParsingError::IncompleteHeader);
                } else if slice.len() < frame.len() {
                    return Err(WsParsingError::IncompleteMessage(frame.len() - slice.len()));
                }
                frame.header_size() + frame.data_len()
            };
            let (frame_slice, remainder) = slice.split_at(frame_size);
            Ok((
                unsafe { Self::from_slice_unchecked(frame_slice) },
                remainder,
            ))
        }
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
    pub fn opcode(&self) -> Opcode {
        let numeric = self.fin_rsv_opcode & 0xf;
        match numeric {
            0x8 => Opcode::Close,
            0x9 => Opcode::Ping,
            0xA => Opcode::Pong,
            0x1 => Opcode::Text,
            0x2 => Opcode::Binary,
            x => Opcode::Invalid(x),
        }
    }
    pub fn is_close(&self) -> bool {
        match self.opcode() {
            Opcode::Close => true,
            _ => false,
        }
    }
    pub fn close_code(&self) -> Option<u16> {
        if self.is_close() && self.data_len() >= 2 {
            let data = self.unmasked_data();
            Some(u16::from_be_bytes([data[0], data[1]]))
        } else {
            None
        }
    }
    pub fn ContentLengthByte(&self) -> ContentLength {
        match self.mask_len % 128 {
            127 => ContentLength::EightBytes,
            126 => ContentLength::TwoBytes,
            x => ContentLength::OneByte(x),
        }
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
    pub fn has_mask(&self) -> bool {
        self.mask_len >= 1 << 7
    }
    pub fn header_size(&self) -> usize {
        2 + if self.has_mask() { 4 } else { 0 }
            + match self.ContentLengthByte() {
                ContentLength::EightBytes => 8,
                ContentLength::TwoBytes => 2,
                ContentLength::OneByte(_) => 0, //n.b. in the one byte case, the length is embded in the first 2 bytes
            }
    }
    pub fn masked_data(&self) -> &[u8] {
        let offset = self.header_size() - 2;
        &self.dynamic[offset..offset + self.data_len()]
    }
    pub fn unmasked_data(&self) -> &[u8] {
        if self.has_mask() {
            panic!("not implemented yet!");
        } else {
            self.masked_data()
        }
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
            .field("data", &{
                let data = self.masked_data();
                let string = match self.opcode() {
                    Opcode::Binary | Opcode::Invalid(_) => format!("[{} bytes]", data.len()),
                    Opcode::Text => match String::from_utf8(data.to_vec()) {
                        Ok(x) => x.to_owned(),
                        Err(_) => "[malformed string]".to_owned(),
                    },
                    Opcode::Close | Opcode::Ping | Opcode::Pong => {
                        if data.len() >= 2 {
                            format!("[{}]", u16::from_be_bytes([data[0], data[1]]))
                        } else {
                            "[no closing code given]".to_owned()
                        }
                    }
                };
                string
            })
            .finish()
    }
}
