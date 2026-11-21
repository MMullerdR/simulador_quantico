//! Parte comum dos executáveis `shor`, `grover` e `general`: a leitura dos
//! argumentos posicionais, que segue a ordem dos `*.out` do C++
//! (`src/cli/cli_common.cpp`):
//!
//! ```text
//! QUBITS [EXEC_TYPE] [THREADS|GPUS] [BLOCK_SIZE] [REPEAT_COUNT] [GPU_REGION_BITS] [...]
//! ```
//!
//! `EXEC_TYPE` é `0` (CPU serial), `1` (CPU paralela), `2` (GPU) ou `3` (híbrido).
//! Os tipos `2` e `3` ainda não existem na versão Rust (fases 4 e 6 de
//! `docs/09`). `BLOCK_SIZE`, `REPEAT_COUNT` e `GPU_REGION_BITS` só valem para GPU;
//! são lidos (e têm que ser inteiros) mas ignorados nos backends de CPU.
//!
//! Diferenças deliberadas em relação ao C++: argumento inválido sai com código
//! diferente de zero (2 para uso, 1 para erro de execução) em vez de imprimir a
//! mensagem e sair com 0, e a semente do gerador pode ser fixada com a variável de
//! ambiente `HGM_SEED` (o C++ sempre usa `time ^ pid`).

use std::fmt;
use std::process::ExitCode;
use std::str::FromStr;

use hgm_core::HgmError;
use hgm_cpu::Tuning;
use hgm_engine::{BackendKind, GpuTuning};

/// Região e coalescida padrão da CPU paralela (`TuningDefaults` do C++).
pub const DEFAULT_CPU_REGION_BITS: u32 = 14;
pub const DEFAULT_CPU_COALESCED_BITS: u32 = 11;

#[derive(Debug)]
pub enum CliError {
    /// Argumentos errados: a mensagem diz o que corrigir.
    Usage(String),
    /// A simulação falhou.
    Run(HgmError),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CliError::Usage(message) => f.write_str(message),
            CliError::Run(error) => write!(f, "erro: {error}"),
        }
    }
}

impl From<HgmError> for CliError {
    fn from(error: HgmError) -> Self {
        CliError::Run(error)
    }
}

/// Lê o argumento `index` como número, ou `None` se não foi dado.
pub fn parse_arg<T: FromStr>(
    args: &[String],
    index: usize,
    name: &str,
) -> Result<Option<T>, CliError> {
    match args.get(index) {
        None => Ok(None),
        Some(text) => text
            .trim()
            .parse()
            .map(Some)
            .map_err(|_| CliError::Usage(format!("{name} inválido: `{text}`"))),
    }
}

/// Número de qubits (`args[0]`), obrigatório.
pub fn parse_qubits(args: &[String]) -> Result<usize, CliError> {
    parse_arg::<usize>(args, 0, "QUBITS")?
        .ok_or_else(|| CliError::Usage("You need to define the execution parameters".to_owned()))
}

/// Backend pedido em `args[1]` (tipo de execução, padrão `default_type`) e
/// `args[2]` (threads). Confere também que `args[3..=5]`, se existirem, são inteiros.
pub fn parse_backend(args: &[String], default_type: i64) -> Result<BackendKind, CliError> {
    let exec_type = parse_arg::<i64>(args, 1, "EXEC_TYPE")?.unwrap_or(default_type);
    for (index, name) in [
        (3, "BLOCK_SIZE"),
        (4, "REPEAT_COUNT"),
        (5, "GPU_REGION_BITS"),
    ] {
        parse_arg::<i64>(args, index, name)?;
    }

    match exec_type {
        0 => Ok(BackendKind::CpuSerial),
        1 => {
            let threads = parse_arg::<usize>(args, 2, "THREADS")?.unwrap_or(1);
            Ok(BackendKind::CpuParallel(Tuning {
                threads,
                coalesced_bits: DEFAULT_CPU_COALESCED_BITS,
                region_bits: DEFAULT_CPU_REGION_BITS,
            }))
        }
        // BLOCK_SIZE (threads por bloco do CUDA) e REPEAT_COUNT não têm correspondente
        // no wgpu e são ignorados; GPU_REGION_BITS vale como no C++. GPUS (`args[2]`)
        // também é ignorado: o Rust usa um dispositivo só.
        2 => {
            let mut tuning = GpuTuning::default();
            if let Some(bits) = parse_arg::<u32>(args, 5, "GPU_REGION_BITS")? {
                tuning.region_bits = bits;
            }
            Ok(BackendKind::Gpu(tuning))
        }
        3 => Err(CliError::Usage(
            "Execution type 3 (t_HYBRID) ainda não existe na versão Rust (fase 6 de docs/09); \
             use 0 (CPU), 1 (CPU paralela) ou 2 (GPU)"
                .to_owned(),
        )),
        other => Err(CliError::Usage(format!("Invalid execution type: {other}"))),
    }
}

/// Semente do gerador: `HGM_SEED` se definida; senão `time ^ pid`, para dois
/// processos lançados no mesmo segundo não repetirem a sequência (item 18 de
/// `docs/07`).
pub fn seed_from_environment() -> Result<u32, CliError> {
    if let Ok(text) = std::env::var("HGM_SEED") {
        return text.trim().parse().map_err(|_| {
            CliError::Usage(format!(
                "HGM_SEED inválida: `{text}` (esperado um inteiro de 32 bits sem sinal)"
            ))
        });
    }
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    Ok((seconds as u32) ^ std::process::id())
}

/// Converte o resultado de `run` em código de saída, imprimindo o erro em stderr.
pub fn finish(result: Result<(), CliError>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error @ CliError::Usage(_)) => {
            eprintln!("{error}");
            ExitCode::from(2)
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn qubits_are_required_and_numeric() {
        assert_eq!(parse_qubits(&args(&["12"])).unwrap(), 12);
        assert!(matches!(parse_qubits(&args(&[])), Err(CliError::Usage(_))));
        assert!(matches!(
            parse_qubits(&args(&["x"])),
            Err(CliError::Usage(_))
        ));
        assert!(matches!(
            parse_qubits(&args(&["-3"])),
            Err(CliError::Usage(_))
        ));
    }

    #[test]
    fn backend_follows_the_cpp_positions() {
        assert_eq!(
            parse_backend(&args(&["10"]), 0).unwrap(),
            BackendKind::CpuSerial
        );
        match parse_backend(&args(&["10", "1", "4"]), 0).unwrap() {
            BackendKind::CpuParallel(tuning) => {
                assert_eq!(tuning.threads, 4);
                assert_eq!(tuning.region_bits, 14);
                assert_eq!(tuning.coalesced_bits, 11);
            }
            other => panic!("backend inesperado: {other:?}"),
        }
        // Sem THREADS, 1 thread (como o `TuningDefaults` do C++).
        match parse_backend(&args(&["10", "1"]), 0).unwrap() {
            BackendKind::CpuParallel(tuning) => assert_eq!(tuning.threads, 1),
            other => panic!("backend inesperado: {other:?}"),
        }
    }

    #[test]
    fn gpu_type_is_accepted_and_hybrid_garbage_are_usage_errors() {
        assert_eq!(
            parse_backend(&args(&["10", "2", "1"]), 0).unwrap(),
            BackendKind::Gpu(GpuTuning::default())
        );
        match parse_backend(&args(&["10", "2", "1", "512", "1", "9"]), 0).unwrap() {
            BackendKind::Gpu(tuning) => assert_eq!(tuning.region_bits, 9),
            other => panic!("backend inesperado: {other:?}"),
        }
        assert!(matches!(
            parse_backend(&args(&["10", "2", "1", "512", "1", "x"]), 0),
            Err(CliError::Usage(_))
        ));
        assert!(matches!(
            parse_backend(&args(&["10", "3"]), 0),
            Err(CliError::Usage(_))
        ));
        assert!(matches!(
            parse_backend(&args(&["10", "9"]), 0),
            Err(CliError::Usage(_))
        ));
        assert!(matches!(
            parse_backend(&args(&["10", "1", "2", "x"]), 0),
            Err(CliError::Usage(_))
        ));
    }
}
