use std::time::Instant;
fn main() -> wasmtime::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let (manifest, wasm, tool, params) = (std::fs::read_to_string(&a[1])?, std::fs::read(&a[2])?, &a[3], &a[4]);
    let engine = wasm_host::engine();
    let t = Instant::now();
    let mut l = wasm_host::load(&engine, &manifest, &wasm)?;
    let load = t.elapsed();
    let t = Instant::now();
    let r = l.prepare(tool, params)?;
    println!("load+instantiate {load:?}, first call {:?}, result {r:?}", t.elapsed());
    Ok(())
}
