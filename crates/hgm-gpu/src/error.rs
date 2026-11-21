use thiserror::Error;

#[derive(Debug, Error)]
pub enum GpuError {
    /// Nenhum adaptador utilizável (sem driver Vulkan/Metal/DX12, ou só software
    /// quando se pediu hardware).
    #[error("nenhum adaptador de GPU disponível: {0}")]
    NoAdapter(String),

    #[error("falha ao abrir o dispositivo: {0}")]
    Device(String),

    /// A GPU não tem memória para o estado (ou para os blocos auxiliares da
    /// multiplicação modular).
    #[error("sem memória na GPU: {0}")]
    OutOfMemory(String),

    #[error("não suportado na GPU: {0}")]
    Unsupported(String),

    #[error("erro ao ler resultados da GPU: {0}")]
    Readback(String),

    #[error("o dispositivo de GPU reportou um erro: {0}")]
    Validation(String),
}

pub type Result<T> = std::result::Result<T, GpuError>;
