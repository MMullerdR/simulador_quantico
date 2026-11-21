//! Erros do núcleo. No C++ vários destes casos eram `exit(1)`, ponteiro nulo ou
//! comportamento indefinido; aqui viram valores de erro.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum HgmError {
    #[error("passo {step}: token `{token}` inválido ({reason})")]
    BadToken {
        step: usize,
        token: String,
        reason: &'static str,
    },

    #[error("porta desconhecida `{0}`")]
    UnknownGate(String),

    #[error("o passo {step} tem {found} qubits, mas o circuito tem {expected}")]
    InconsistentQubits {
        step: usize,
        found: usize,
        expected: usize,
    },

    #[error("{qubits} qubits excede o limite de {max}")]
    TooManyQubits { qubits: usize, max: usize },

    #[error("sem memória para um estado de {qubits} qubits")]
    OutOfMemory { qubits: usize },

    #[error("estado de {qubits} qubits precisa de {expected} bytes, recebi {found}")]
    BadStateSize {
        qubits: usize,
        expected: usize,
        found: usize,
    },

    #[error("o programa tem {program} qubits e o estado tem {state}")]
    QubitMismatch { program: usize, state: usize },

    #[error("porta compilada fora do estado: {0}")]
    PtOutOfRange(&'static str),

    #[error("argumento inválido: {0}")]
    InvalidArgument(String),

    #[error("o ramo escolhido tem probabilidade zero e não pode ser normalizado")]
    ZeroProbability,

    #[error("backend: {0}")]
    Backend(String),
}

pub type Result<T> = std::result::Result<T, HgmError>;
