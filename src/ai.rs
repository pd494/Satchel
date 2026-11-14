use serde::Serialize;
use serde_json::Value;
use wasm_bindgen::prelude::*;
use worker::js_sys::Promise;
use worker::wasm_bindgen_futures::JsFuture;
use worker::{Ai, Result};

#[wasm_bindgen]
extern "C" {
    type Binding;

    /// `Ai::run` has no options argument, which AI Gateway routing needs.
    #[wasm_bindgen(method, catch)]
    fn run(
        this: &Binding,
        model: &str,
        input: JsValue,
        options: JsValue,
    ) -> std::result::Result<Promise, JsValue>;
}

pub async fn run(ai: &Ai, model: &str, input: &Value, options: &Value) -> Result<Value> {
    let json = serde_wasm_bindgen::Serializer::json_compatible();
    let binding: &Binding = ai.as_ref().unchecked_ref();
    let promise = binding.run(model, input.serialize(&json)?, options.serialize(&json)?)?;
    Ok(serde_wasm_bindgen::from_value(
        JsFuture::from(promise).await?,
    )?)
}
