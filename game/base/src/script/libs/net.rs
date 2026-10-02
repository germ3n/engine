use crate::network::usermessage::{hash_usermessage_name, UserMsgWriter};
use mlua::Lua;
use r#macro::document;
use std::sync::mpsc::Sender;

#[document(
    parent = "net",
    name = "writer",
    realm = "shared",
    summary = "Creates a usermessage writer.",
    params = {
        capacity = { ty = "number", desc = "Optional byte capacity.", optional = true },
    },
    returns = { ty = "UserMsgWriter", desc = "Writer passed to net.send. Methods: write_u8, write_i8, write_u16, write_i16, write_u32, write_i32, write_u64, write_i64, write_f32, write_f64." },
    example = "local writer = net.writer()\nwriter:write_u8(1)\nnet.send(\"hit\", writer)",
    see_also = "net.send",
)]
fn net_writer() {}

#[document(
    parent = "net",
    name = "send",
    realm = "shared",
    summary = "Sends a usermessage to the other side of the connection.",
    params = {
        name = { ty = "string", desc = "Message name. The receiver looks it up with net.hash." },
        writer = { ty = "UserMsgWriter", desc = "Bytes from net.writer." },
    },
    returns = { ty = "nil", desc = "" },
    example = "net.send(\"hit\", net.writer())",
    see_also = "net.writer, net.hash",
)]
fn net_send() {}

pub fn register_net_lib(lua: &Lua, usermsg_sender: Sender<(u32, Vec<u8>)>) {
    let net_table: mlua::Table = lua
        .globals()
        .get("net")
        .expect("[net] Couldn't get net table");

    let writer_func = lua
        .create_function(move |_, capacity: Option<u32>| {
            let writer = if let Some(cap) = capacity {
                UserMsgWriter::with_capacity(cap as usize)
            } else {
                UserMsgWriter::new()
            };
            Ok(writer)
        })
        .expect("[net] Failed to create writer function");

    net_table
        .set("writer", writer_func)
        .expect("[net] Failed to set writer");

    let send_func = lua
        .create_function(
            move |_, (msg_name, writer_data): (String, mlua::AnyUserData)| {
                let msg_hash = hash_usermessage_name(msg_name.as_str());
                let writer = writer_data.borrow::<UserMsgWriter>()?;
                let _ = usermsg_sender.send((msg_hash, writer.bytes().to_vec()));

                Ok(())
            },
        )
        .expect("[net] Failed to create send function");

    net_table
        .set("send", send_func)
        .expect("[net] Failed to set send function");
}
