use super::{Error, Result};
use js_sys::Reflect;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;

pub(crate) fn generate_entity_id(byte_len: usize) -> Result<String> {
    let crypto = Reflect::get(&js_sys::global(), &JsValue::from_str("crypto"))?
        .dyn_into::<web_sys::Crypto>()
        .map_err(|_| Error::RustError("failed to access global crypto".to_owned()))?;
    let mut bytes = vec![0u8; byte_len];
    crypto
        .get_random_values_with_u8_array(&mut bytes)
        .map_err(Error::from)?;

    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}
