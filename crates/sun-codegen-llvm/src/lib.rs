//! LLVM backend scaffold. Requires LLVM 18 development libraries.

use inkwell::context::Context;
use inkwell::targets::{InitializationConfig, Target};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BackendError {
    #[error("native LLVM target initialization failed: {0}")]
    TargetInitialization(String),
}

pub struct LlvmBackend { context: Context }

impl LlvmBackend {
    pub fn initialize() -> Result<Self, BackendError> {
        Target::initialize_native(&InitializationConfig::default())
            .map_err(BackendError::TargetInitialization)?;
        Ok(Self { context: Context::create() })
    }

    pub fn create_module<'ctx>(&'ctx self, name: &str) -> inkwell::module::Module<'ctx> {
        self.context.create_module(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_an_llvm_module() {
        let backend = LlvmBackend::initialize().expect("LLVM native target should initialize");
        let module = backend.create_module("sun_test");
        assert_eq!(module.get_name().to_str().unwrap(), "sun_test");
    }
}
