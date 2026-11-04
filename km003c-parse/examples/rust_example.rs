// rust_example.rs — use km003c-parse as a normal Rust crate.
//
// Run from the workspace root: cargo run -p km003c-parse --example rust_example
use km003c_parse::parse_pd_wire;

fn main() {
    // Source_Capabilities with one PPS APDO (3.30-11.00 V, 5.00 A).
    let mut wire = vec![0x01u8, 0x11];
    wire.extend_from_slice(&0xC0DC_2164u32.to_le_bytes());

    match parse_pd_wire(&wire) {
        Ok(decoded) => {
            println!("{}", decoded.to_json_string());
        }
        Err(error) => eprintln!("decode failed: {error}"),
    }
}
