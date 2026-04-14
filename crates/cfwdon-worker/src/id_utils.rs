use super::{Error, Result};
use wasm_bindgen::JsCast;
use web_sys::WorkerGlobalScope;

pub(crate) fn generate_entity_id(byte_len: usize) -> Result<String> {
    let global = js_sys::global()
        .dyn_into::<WorkerGlobalScope>()
        .map_err(|_| Error::RustError("failed to access WorkerGlobalScope".to_owned()))?;
    let mut bytes = vec![0u8; byte_len];
    global
        .crypto()
        .map_err(Error::from)?
        .get_random_values_with_u8_array(&mut bytes)
        .map_err(Error::from)?;

    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}
