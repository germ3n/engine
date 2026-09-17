use mlua::{UserData, UserDataMethods};

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
        Self { data: Vec::with_capacity(capacity) }
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
            if this.idx >= this.data.len() { return Ok(None); }
            let val = this.data[this.idx];
            this.idx += size_of::<u8>();
            Ok(Some(val))
        });

        methods.add_method_mut("read_i8", |_, _this, ()| {
            /*if this.idx >= this.data.len() { return Ok(None); }
            let val = this.data[this.idx];
            this.idx += size_of::<i8>();
            Ok(Some(val)) */
            todo!();
            Ok(mlua::Nil)
        });

        methods.add_method_mut("read_u16", |_, this, ()| {
            let end_idx = this.idx + size_of::<u16>();
            if end_idx > this.data.len() { return Ok(None); }
            let bytes = this.data[this.idx..end_idx].try_into().unwrap();
            this.idx = end_idx;
            Ok(Some(u16::from_le_bytes(bytes)))
        });

        methods.add_method_mut("read_i16", |_, this, ()| {
            let end_idx = this.idx + size_of::<i16>();
            if end_idx > this.data.len() { return Ok(None); }
            let bytes = this.data[this.idx..end_idx].try_into().unwrap();
            this.idx = end_idx;
            Ok(Some(i16::from_le_bytes(bytes)))
        });
        
        methods.add_method_mut("read_u32", |_, this, ()| {
            let end_idx = this.idx + size_of::<u32>();
            if end_idx > this.data.len() { return Ok(None); }
            let bytes = this.data[this.idx..end_idx].try_into().unwrap();
            this.idx = end_idx;
            Ok(Some(u32::from_le_bytes(bytes)))
        });

        methods.add_method_mut("read_i32", |_, this, ()| {
            let end_idx = this.idx + size_of::<i32>();
            if end_idx > this.data.len() { return Ok(None); }
            let bytes = this.data[this.idx..end_idx].try_into().unwrap();
            this.idx = end_idx;
            Ok(Some(i32::from_le_bytes(bytes)))
        });

        methods.add_method_mut("read_u64", |_, this, ()| {
            let end_idx = this.idx + size_of::<u64>();
            if end_idx > this.data.len() { return Ok(None); }
            let bytes = this.data[this.idx..end_idx].try_into().unwrap();
            this.idx = end_idx;
            Ok(Some(u64::from_le_bytes(bytes)))
        });

        methods.add_method_mut("read_i64", |_, this, ()| {
            let end_idx = this.idx + size_of::<i64>();
            if end_idx > this.data.len() { return Ok(None); }
            let bytes = this.data[this.idx..end_idx].try_into().unwrap();
            this.idx = end_idx;
            Ok(Some(i64::from_le_bytes(bytes)))
        });

        methods.add_method_mut("read_f32", |_, this, ()| {
            let end_idx = this.idx + size_of::<f32>();
            if end_idx > this.data.len() { return Ok(None); }
            let bytes = this.data[this.idx..end_idx].try_into().unwrap();
            this.idx = end_idx;
            Ok(Some(f32::from_le_bytes(bytes)))
        });

        methods.add_method_mut("read_f64", |_, this, ()| {
            let end_idx = this.idx + size_of::<f64>();
            if end_idx > this.data.len() { return Ok(None); }
            let bytes = this.data[this.idx..end_idx].try_into().unwrap();
            this.idx = end_idx;
            Ok(Some(f64::from_le_bytes(bytes)))
        });
    }
}