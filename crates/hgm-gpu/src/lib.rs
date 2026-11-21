//! Backend de GPU portátil (wgpu + WGSL). **Ainda vazio.**
//!
//! A fase 2 de `docs/09-plano-migracao-rust.md` é o spike go/no-go: kernel de porta
//! densa e de fase fundida em WGSL, medição com soma em `f32` fechada em `f64`, e
//! comparação com o CUDA na A4500 (e em GPUs de outros fornecedores quando houver
//! hardware, ver `docs/11`). Este crate existe agora só para fixar o lugar no
//! workspace e a direção da dependência (`engine -> gpu -> core`).
//!
//! Limitações já conhecidas do WGSL que o desenho precisa respeitar (`docs/09` §5):
//! sem `f64` portátil, índice `u32` (logo no máximo 32 qubits na GPU), memória de
//! workgroup padrão de 16 KiB (o kernel CUDA usa 32 KiB) e sem memória fixada (o
//! modo híbrido é o risco).

/// Estado do crate, para o CLI/testes poderem dizer a verdade sobre ele.
pub const STATUS: &str = "não implementado (fase 2 do plano de migração)";
