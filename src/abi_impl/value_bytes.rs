use vm::{Value, VmError};

pub(crate) fn bytes_to_value(bytes: Vec<u8>) -> Value {
    Value::bytes(bytes)
}

pub(crate) fn value_to_bytes<'a>(value: &'a Value, _label: &str) -> Result<&'a [u8], VmError> {
    match value {
        Value::Bytes(values) => Ok(values.as_slice()),
        _ => Err(VmError::TypeMismatch("bytes")),
    }
}
