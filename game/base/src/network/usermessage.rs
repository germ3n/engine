use mlua::{UserData, UserDataMethods};
use r#macro::document;

#[document(
    kind = "class",
    name = "UserMsgWriter",
    realm = "shared",
    summary = "Bytes for net.send. Writes are little-endian. Created by net.writer.",
    see_also = "net.writer, net.send",
)]
fn usermsg_writer_class() {}

#[document(
    parent = "UserMsgWriter",
    name = "write_u8",
    kind = "method",
    realm = "shared",
    summary = "Appends an unsigned byte.",
    params = { value = { ty = "number", desc = "0 to 255." } },
)]
fn usermsg_write_u8() {}

#[document(
    parent = "UserMsgWriter",
    name = "write_i8",
    kind = "method",
    realm = "shared",
    summary = "Appends a signed byte.",
    params = { value = { ty = "number", desc = "-128 to 127." } },
)]
fn usermsg_write_i8() {}

#[document(
    parent = "UserMsgWriter",
    name = "write_u16",
    kind = "method",
    realm = "shared",
    summary = "Appends an unsigned 16-bit integer.",
    params = { value = { ty = "number", desc = "0 to 65535." } },
)]
fn usermsg_write_u16() {}

#[document(
    parent = "UserMsgWriter",
    name = "write_i16",
    kind = "method",
    realm = "shared",
    summary = "Appends a signed 16-bit integer.",
    params = { value = { ty = "number", desc = "Signed 16-bit value." } },
)]
fn usermsg_write_i16() {}

#[document(
    parent = "UserMsgWriter",
    name = "write_u32",
    kind = "method",
    realm = "shared",
    summary = "Appends an unsigned 32-bit integer.",
    params = { value = { ty = "number", desc = "Unsigned 32-bit value." } },
)]
fn usermsg_write_u32() {}

#[document(
    parent = "UserMsgWriter",
    name = "write_i32",
    kind = "method",
    realm = "shared",
    summary = "Appends a signed 32-bit integer.",
    params = { value = { ty = "number", desc = "Signed 32-bit value." } },
)]
fn usermsg_write_i32() {}

#[document(
    parent = "UserMsgWriter",
    name = "write_u64",
    kind = "method",
    realm = "shared",
    summary = "Appends an unsigned 64-bit integer.",
    params = { value = { ty = "number", desc = "Unsigned 64-bit value." } },
)]
fn usermsg_write_u64() {}

#[document(
    parent = "UserMsgWriter",
    name = "write_i64",
    kind = "method",
    realm = "shared",
    summary = "Appends a signed 64-bit integer.",
    params = { value = { ty = "number", desc = "Signed 64-bit value." } },
)]
fn usermsg_write_i64() {}

#[document(
    parent = "UserMsgWriter",
    name = "write_f32",
    kind = "method",
    realm = "shared",
    summary = "Appends a 32-bit float.",
    params = { value = { ty = "number", desc = "The float." } },
)]
fn usermsg_write_f32() {}

#[document(
    parent = "UserMsgWriter",
    name = "write_f64",
    kind = "method",
    realm = "shared",
    summary = "Appends a 64-bit float.",
    params = { value = { ty = "number", desc = "The float." } },
)]
fn usermsg_write_f64() {}

#[document(
    kind = "class",
    name = "UserMsgReader",
    realm = "shared",
    summary = "The userdata passed to a usermessage callback. Reads are little-endian and return nil when the buffer runs out.",
    see_also = "net.add_callback",
)]
fn usermsg_reader_class() {}

#[document(
    parent = "UserMsgReader",
    name = "read_u8",
    kind = "method",
    realm = "shared",
    summary = "Reads an unsigned byte.",
    returns = { ty = "number", desc = "The byte, or nil at the end of the buffer." },
)]
fn usermsg_read_u8() {}

#[document(
    parent = "UserMsgReader",
    name = "read_i8",
    kind = "method",
    realm = "shared",
    summary = "Reads a signed byte.",
    returns = { ty = "number", desc = "The byte, or nil at the end of the buffer." },
)]
fn usermsg_read_i8() {}

#[document(
    parent = "UserMsgReader",
    name = "read_u16",
    kind = "method",
    realm = "shared",
    summary = "Reads an unsigned 16-bit integer.",
    returns = { ty = "number", desc = "The integer, or nil when fewer than 2 bytes remain." },
)]
fn usermsg_read_u16() {}

#[document(
    parent = "UserMsgReader",
    name = "read_i16",
    kind = "method",
    realm = "shared",
    summary = "Reads a signed 16-bit integer.",
    returns = { ty = "number", desc = "The integer, or nil when fewer than 2 bytes remain." },
)]
fn usermsg_read_i16() {}

#[document(
    parent = "UserMsgReader",
    name = "read_u32",
    kind = "method",
    realm = "shared",
    summary = "Reads an unsigned 32-bit integer.",
    returns = { ty = "number", desc = "The integer, or nil when fewer than 4 bytes remain." },
)]
fn usermsg_read_u32() {}

#[document(
    parent = "UserMsgReader",
    name = "read_i32",
    kind = "method",
    realm = "shared",
    summary = "Reads a signed 32-bit integer.",
    returns = { ty = "number", desc = "The integer, or nil when fewer than 4 bytes remain." },
)]
fn usermsg_read_i32() {}

#[document(
    parent = "UserMsgReader",
    name = "read_u64",
    kind = "method",
    realm = "shared",
    summary = "Reads an unsigned 64-bit integer.",
    returns = { ty = "number", desc = "The integer, or nil when fewer than 8 bytes remain." },
)]
fn usermsg_read_u64() {}

#[document(
    parent = "UserMsgReader",
    name = "read_i64",
    kind = "method",
    realm = "shared",
    summary = "Reads a signed 64-bit integer.",
    returns = { ty = "number", desc = "The integer, or nil when fewer than 8 bytes remain." },
)]
fn usermsg_read_i64() {}

#[document(
    parent = "UserMsgReader",
    name = "read_f32",
    kind = "method",
    realm = "shared",
    summary = "Reads a 32-bit float.",
    returns = { ty = "number", desc = "The float, or nil when fewer than 4 bytes remain." },
)]
fn usermsg_read_f32() {}

#[document(
    parent = "UserMsgReader",
    name = "read_f64",
    kind = "method",
    realm = "shared",
    summary = "Reads a 64-bit float.",
    returns = { ty = "number", desc = "The float, or nil when fewer than 8 bytes remain." },
)]
fn usermsg_read_f64() {}

pub fn hash_usermessage_name(name: &str) -> u32 {
    let bytes = name.as_bytes();
    let mut hash: u32 = 2166136261;

    for idx in 0..bytes.len() {
        hash ^= bytes[idx] as u32;
        hash = hash.wrapping_mul(16777619);
    }

    hash
}

pub struct UserMsgWriter {
    data: Vec<u8>,
}

impl UserMsgWriter {
    pub fn new() -> Self {
        Self { data: Vec::new() }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            data: Vec::with_capacity(capacity),
        }
    }

    pub fn bytes(&self) -> &[u8] {
        &self.data
    }

    pub fn write_u8(&mut self, val: u8) {
        self.data.push(val);
    }

    pub fn write_i8(&mut self, val: i8) {
        self.data.extend_from_slice(&val.to_le_bytes());
    }

    pub fn write_u16(&mut self, val: u16) {
        self.data.extend_from_slice(&val.to_le_bytes());
    }

    pub fn write_i16(&mut self, val: i16) {
        self.data.extend_from_slice(&val.to_le_bytes());
    }

    pub fn write_u32(&mut self, val: u32) {
        self.data.extend_from_slice(&val.to_le_bytes());
    }

    pub fn write_i32(&mut self, val: i32) {
        self.data.extend_from_slice(&val.to_le_bytes());
    }

    pub fn write_u64(&mut self, val: u64) {
        self.data.extend_from_slice(&val.to_le_bytes());
    }

    pub fn write_i64(&mut self, val: i64) {
        self.data.extend_from_slice(&val.to_le_bytes());
    }

    pub fn write_f32(&mut self, val: f32) {
        self.data.extend_from_slice(&val.to_le_bytes());
    }

    pub fn write_f64(&mut self, val: f64) {
        self.data.extend_from_slice(&val.to_le_bytes());
    }
}

impl UserData for UserMsgWriter {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method_mut("write_u8", |_, this, value: u8| {
            this.data.push(value);
            Ok(())
        });

        methods.add_method_mut("write_i8", |_, this, value: i8| {
            this.data.extend_from_slice(&value.to_le_bytes());
            Ok(())
        });

        methods.add_method_mut("write_u16", |_, this, value: u16| {
            this.data.extend_from_slice(&value.to_le_bytes());
            Ok(())
        });

        methods.add_method_mut("write_i16", |_, this, value: i16| {
            this.data.extend_from_slice(&value.to_le_bytes());
            Ok(())
        });

        methods.add_method_mut("write_u32", |_, this, value: u32| {
            this.data.extend_from_slice(&value.to_le_bytes());
            Ok(())
        });

        methods.add_method_mut("write_i32", |_, this, value: i32| {
            this.data.extend_from_slice(&value.to_le_bytes());
            Ok(())
        });

        methods.add_method_mut("write_u64", |_, this, value: u64| {
            this.data.extend_from_slice(&value.to_le_bytes());
            Ok(())
        });

        methods.add_method_mut("write_i64", |_, this, value: i64| {
            this.data.extend_from_slice(&value.to_le_bytes());
            Ok(())
        });

        methods.add_method_mut("write_f32", |_, this, value: f32| {
            this.data.extend_from_slice(&value.to_le_bytes());
            Ok(())
        });

        methods.add_method_mut("write_f64", |_, this, value: f64| {
            this.data.extend_from_slice(&value.to_le_bytes());
            Ok(())
        });
    }
}

pub struct UserMsgReader {
    data: Vec<u8>,
    idx: usize,
}

impl UserMsgReader {
    pub fn new(data: Vec<u8>) -> Self {
        Self { data, idx: 0 }
    }

    pub fn read_u8(&mut self) -> Option<u8> {
        if self.idx >= self.data.len() {
            return None;
        }

        let val = self.data[self.idx];
        self.idx += size_of::<u8>();
        Some(val)
    }

    pub fn read_i8(&mut self) -> Option<i8> {
        self.read_u8().map(|v| v as i8)
    }

    pub fn read_u16(&mut self) -> Option<u16> {
        let end_idx = self.idx + size_of::<u16>();
        if end_idx > self.data.len() {
            return None;
        }

        let bytes = self.data[self.idx..end_idx].try_into().unwrap();
        self.idx = end_idx;
        Some(u16::from_le_bytes(bytes))
    }

    pub fn read_i16(&mut self) -> Option<i16> {
        let end_idx = self.idx + size_of::<i16>();
        if end_idx > self.data.len() {
            return None;
        }

        let bytes = self.data[self.idx..end_idx].try_into().unwrap();
        self.idx = end_idx;
        Some(i16::from_le_bytes(bytes))
    }

    pub fn read_u32(&mut self) -> Option<u32> {
        let end_idx = self.idx + size_of::<u32>();
        if end_idx > self.data.len() {
            return None;
        }

        let bytes = self.data[self.idx..end_idx].try_into().unwrap();
        self.idx = end_idx;
        Some(u32::from_le_bytes(bytes))
    }

    pub fn read_i32(&mut self) -> Option<i32> {
        let end_idx = self.idx + size_of::<i32>();
        if end_idx > self.data.len() {
            return None;
        }

        let bytes = self.data[self.idx..end_idx].try_into().unwrap();
        self.idx = end_idx;
        Some(i32::from_le_bytes(bytes))
    }

    pub fn read_u64(&mut self) -> Option<u64> {
        let end_idx = self.idx + size_of::<u64>();
        if end_idx > self.data.len() {
            return None;
        }

        let bytes = self.data[self.idx..end_idx].try_into().unwrap();
        self.idx = end_idx;
        Some(u64::from_le_bytes(bytes))
    }

    pub fn read_i64(&mut self) -> Option<i64> {
        let end_idx = self.idx + size_of::<i64>();
        if end_idx > self.data.len() {
            return None;
        }

        let bytes = self.data[self.idx..end_idx].try_into().unwrap();
        self.idx = end_idx;
        Some(i64::from_le_bytes(bytes))
    }

    pub fn read_f32(&mut self) -> Option<f32> {
        let end_idx = self.idx + size_of::<f32>();
        if end_idx > self.data.len() {
            return None;
        }

        let bytes = self.data[self.idx..end_idx].try_into().unwrap();
        self.idx = end_idx;
        Some(f32::from_le_bytes(bytes))
    }

    pub fn read_f64(&mut self) -> Option<f64> {
        let end_idx = self.idx + size_of::<f64>();
        if end_idx > self.data.len() {
            return None;
        }

        let bytes = self.data[self.idx..end_idx].try_into().unwrap();
        self.idx = end_idx;
        Some(f64::from_le_bytes(bytes))
    }
}

impl UserData for UserMsgReader {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method_mut("read_u8", |_, this, ()| {
            if this.idx >= this.data.len() {
                return Ok(None);
            }
            let val = this.data[this.idx];
            this.idx += size_of::<u8>();
            Ok(Some(val))
        });

        methods.add_method_mut("read_i8", |_, this, ()| {
            /*if this.idx >= this.data.len() { return Ok(None); }
            let val = this.data[this.idx];
            this.idx += size_of::<i8>();
            Ok(Some(val)) */
            if this.idx >= this.data.len() {
                return Ok(None);
            }

            let val = this.data[this.idx] as i8;
            this.idx += size_of::<u8>();

            Ok(Some(val))
        });

        methods.add_method_mut("read_u16", |_, this, ()| {
            let end_idx = this.idx + size_of::<u16>();
            if end_idx > this.data.len() {
                return Ok(None);
            }
            let bytes = this.data[this.idx..end_idx].try_into().unwrap();
            this.idx = end_idx;
            Ok(Some(u16::from_le_bytes(bytes)))
        });

        methods.add_method_mut("read_i16", |_, this, ()| {
            let end_idx = this.idx + size_of::<i16>();
            if end_idx > this.data.len() {
                return Ok(None);
            }
            let bytes = this.data[this.idx..end_idx].try_into().unwrap();
            this.idx = end_idx;
            Ok(Some(i16::from_le_bytes(bytes)))
        });

        methods.add_method_mut("read_u32", |_, this, ()| {
            let end_idx = this.idx + size_of::<u32>();
            if end_idx > this.data.len() {
                return Ok(None);
            }
            let bytes = this.data[this.idx..end_idx].try_into().unwrap();
            this.idx = end_idx;
            Ok(Some(u32::from_le_bytes(bytes)))
        });

        methods.add_method_mut("read_i32", |_, this, ()| {
            let end_idx = this.idx + size_of::<i32>();
            if end_idx > this.data.len() {
                return Ok(None);
            }
            let bytes = this.data[this.idx..end_idx].try_into().unwrap();
            this.idx = end_idx;
            Ok(Some(i32::from_le_bytes(bytes)))
        });

        methods.add_method_mut("read_u64", |_, this, ()| {
            let end_idx = this.idx + size_of::<u64>();
            if end_idx > this.data.len() {
                return Ok(None);
            }
            let bytes = this.data[this.idx..end_idx].try_into().unwrap();
            this.idx = end_idx;
            Ok(Some(u64::from_le_bytes(bytes)))
        });

        methods.add_method_mut("read_i64", |_, this, ()| {
            let end_idx = this.idx + size_of::<i64>();
            if end_idx > this.data.len() {
                return Ok(None);
            }
            let bytes = this.data[this.idx..end_idx].try_into().unwrap();
            this.idx = end_idx;
            Ok(Some(i64::from_le_bytes(bytes)))
        });

        methods.add_method_mut("read_f32", |_, this, ()| {
            let end_idx = this.idx + size_of::<f32>();
            if end_idx > this.data.len() {
                return Ok(None);
            }
            let bytes = this.data[this.idx..end_idx].try_into().unwrap();
            this.idx = end_idx;
            Ok(Some(f32::from_le_bytes(bytes)))
        });

        methods.add_method_mut("read_f64", |_, this, ()| {
            let end_idx = this.idx + size_of::<f64>();
            if end_idx > this.data.len() {
                return Ok(None);
            }
            let bytes = this.data[this.idx..end_idx].try_into().unwrap();
            this.idx = end_idx;
            Ok(Some(f64::from_le_bytes(bytes)))
        });
    }
}
