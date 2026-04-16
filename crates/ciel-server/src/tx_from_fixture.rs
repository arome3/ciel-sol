// Reads a fixture transaction.json and prints the base64-encoded transaction to stdout.
// Usage: cargo run --bin tx-from-fixture -- <path-to-transaction.json>
//
// The output can be piped directly into a curl request:
//   TX=$(cargo run --bin tx-from-fixture -- fixtures/drift-exploit/transaction.json)
//   curl -X POST http://localhost:8080/v1/verdict -H "Content-Type: application/json" -d "{\"tx\":\"$TX\"}"

use std::env;
use std::fs;

use serde::Deserialize;

#[derive(Deserialize)]
struct SerializedTransaction {
    data: String,
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: tx-from-fixture <path-to-transaction.json>");
        std::process::exit(1);
    }

    let json = fs::read_to_string(&args[1]).unwrap_or_else(|e| {
        eprintln!("Failed to read {}: {e}", args[1]);
        std::process::exit(1);
    });

    let tx: SerializedTransaction = serde_json::from_str(&json).unwrap_or_else(|e| {
        eprintln!("Failed to parse JSON: {e}");
        std::process::exit(1);
    });

    print!("{}", tx.data);
}
