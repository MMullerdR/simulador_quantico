//! A fronteira entre o motor e quem executa: CPU serial, CPU paralela, GPU (wgpu)
//! e, opcionalmente, CUDA. A trait mora em `hgm-core` (e não em `hgm-engine`)
//! para que `hgm-cpu` e `hgm-gpu` a implementem sem dependência circular:
//! `cli -> algos -> engine -> {cpu, gpu} -> core`.

use crate::circuit::Program;
use crate::error::Result;
use crate::state::StateVector;

pub trait Backend {
    /// Nome para relatórios e tabelas de tempo (ex.: `"t_CPU (Rust, serial)"`).
    fn name(&self) -> &str;

    /// Aplica `program` ao `state`, `repetitions` vezes seguidas (o `iterations`
    /// de `DGM::run` no C++, distinto do `iterations` do parser).
    fn execute(
        &mut self,
        state: &mut StateVector,
        program: &Program,
        repetitions: u32,
    ) -> Result<()>;
}
