use thiserror::Error;

#[derive(Debug, Error)]
pub enum GpuError {
    /// Nenhum adaptador utilizável (sem driver Vulkan/Metal/DX12, ou só software
    /// quando se pediu hardware).
    #[error("nenhum adaptador de GPU disponível: {0}")]
    NoAdapter(String),

    #[error("falha ao abrir o dispositivo: {0}")]
    Device(String),

    #[error(
        "estado de {qubits} qubits ({bytes} bytes) não cabe: limite de buffer {max_buffer} \
         bytes, de binding {max_binding} bytes"
    )]
    StateTooLarge {
        qubits: u32,
        bytes: u64,
        max_buffer: u64,
        max_binding: u64,
    },

    #[error("não suportado na GPU: {0}")]
    Unsupported(String),

    #[error("erro ao ler resultados da GPU: {0}")]
    Readback(String),

    #[error("o dispositivo de GPU reportou um erro: {0}")]
    Validation(String),
}

pub type Result<T> = std::result::Result<T, GpuError>;
