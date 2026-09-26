use mlua::{Lua};
use std::sync::mpsc::Sender;
use crate::network::usermessage::{UserMsgWriter, hash_usermessage_name};

pub fn register_net_lib(
    lua: &Lua,
    usermsg_sender: Sender<(u32, Vec<u8>)>,
) {
    let net_table: mlua::Table = lua.globals().get("net").expect("[net] Couldn't get net table");
                
    let writer_func = lua.create_function(move |_, capacity: Option<u32>| {
        let writer = if let Some(cap) = capacity {
            UserMsgWriter::with_capacity(cap as usize)
        } else {
            UserMsgWriter::new()
        };
        Ok(writer)
    }).expect("[net] Failed to create writer function");

    net_table.set("writer", writer_func).expect("[net] Failed to set writer");

    let send_func = lua.create_function(move |_, (msg_name, writer_data): (String, mlua::AnyUserData)| {
        let msg_hash = hash_usermessage_name(msg_name.as_str());
        let writer = writer_data.borrow::<UserMsgWriter>()?;
        let _ = usermsg_sender.send((msg_hash, writer.bytes().to_vec()));

        Ok(())
    }).expect("[net] Failed to create send function");

    net_table.set("send", send_func).expect("[net] Failed to set send function");
}