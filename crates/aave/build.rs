//! Hands `sol!` each vendored ABI with one repair. alloy-json-abi 1.7.3 declares a user-defined
//! value type named without a namespace inside the interface, where a library struct using it
//! cannot see it, so such a type is given its underlying type. The vendored files stay as published.

use std::error::Error;
use std::fs;
use std::path::PathBuf;

use serde_json::Value;

const ABIS: [(&str, &str); 3] = [
    ("ISpokeV4", "spoke"),
    ("IHubV4", "hub"),
    ("IAaveOracleV4", "oracle"),
];

fn main() -> Result<(), Box<dyn Error>> {
    let out = PathBuf::from(std::env::var("OUT_DIR")?);

    for (interface, module) in ABIS {
        let vendored = format!("abi/{interface}.json");
        println!("cargo::rerun-if-changed={vendored}");

        let mut abi: Value = serde_json::from_str(&fs::read_to_string(&vendored)?)?;
        unscope_value_types(&mut abi);

        let repaired = out.join(format!("{interface}.json"));
        fs::write(&repaired, serde_json::to_string(&abi)?)?;
        fs::write(
            out.join(format!("{module}_bindings.rs")),
            format!("alloy_sol_types::sol!(#[sol(all_derives)] {interface}, {repaired:?});\n"),
        )?;
    }

    Ok(())
}

fn unscope_value_types(value: &mut Value) {
    match value {
        Value::Array(items) => items.iter_mut().for_each(unscope_value_types),
        Value::Object(fields) => {
            let underlying = match (fields.get("internalType"), fields.get("type")) {
                (Some(Value::String(internal)), Some(Value::String(underlying)))
                    if internal != underlying && !internal.contains([' ', '.']) =>
                {
                    Some(underlying.clone())
                }
                _ => None,
            };
            if let Some(underlying) = underlying {
                fields.insert("internalType".to_owned(), Value::String(underlying));
            }
            fields.values_mut().for_each(unscope_value_types);
        }
        _ => {}
    }
}
