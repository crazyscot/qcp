#![allow(missing_docs)]

fn main() {
    built::write_built_file().expect("Failed to acquire build-time information");

    capnpc::CompilerCommand::new()
        .src_prefix("schema")
        .file("schema/session.capnp")
        .default_parent_module(vec!["protocol::session".into()])
        .run()
        .expect("session protocol compiler command");

    capnpc::CompilerCommand::new()
        .src_prefix("schema")
        .file("schema/control.capnp")
        .default_parent_module(vec!["protocol::control".into()])
        .run()
        .expect("control protocol compiler command");
}
