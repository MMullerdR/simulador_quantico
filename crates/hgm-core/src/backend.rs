//! A fronteira entre o motor e quem executa: CPU serial, CPU paralela, GPU (wgpu)
//! e, opcionalmente, CUDA. A trait mora em `hgm-core` (e não em `hgm-engine`)
//! para que `hgm-cpu` e `hgm-gpu` a implementem sem dependência circular:
//! `cli -> algos -> engine -> {cpu, gpu} -> core`.

use crate::circuit::Program;
use crate::error::{HgmError, Result};
use crate::measure::Measurement;
use crate::rng::Mt19937;
use crate::state::StateVector;
use crate::C32;

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

    // --- Estado residente (opcional) ---------------------------------------------
    //
    // Um backend de CPU opera sempre sobre o `StateVector` do host, e os métodos
    // abaixo, com a implementação padrão, bastam. Um backend com o estado residente
    // em outro lugar (a GPU) pode deixar a cópia do host desatualizada entre
    // execuções: nesse caso **precisa** sobrescrever todos eles, porque a
    // implementação padrão lê e escreve a cópia do host.

    /// Chamado uma vez quando o `Simulator` nasce, com o host já em `|0...0>`: um backend
    /// residente cria o estado inicial no próprio dispositivo, sem subir o vetor do host.
    fn initial_state_ready(&mut self, _state: &mut StateVector) -> Result<()> {
        Ok(())
    }

    /// O `Simulator` trocou o estado do host (`set_basis`/`set_state`): qualquer cópia
    /// residente deixou de valer.
    fn host_state_replaced(&mut self) {}

    /// Põe o estado na base `|index>` (`DGM::setMemoryValue` depois de zerar). Um backend
    /// residente pode fazer isso sem alocar nem subir um vetor de `2^n` amplitudes.
    fn set_basis(&mut self, state: &mut StateVector, index: usize) -> Result<()> {
        *state = StateVector::basis(state.qubits(), index)?;
        Ok(())
    }

    /// Deixa `state` (a cópia do host) igual ao estado de verdade.
    fn sync_to_host(&mut self, _state: &mut StateVector) -> Result<()> {
        Ok(())
    }

    /// Mede um qubit e colapsa o estado (`StateVector::measure_qubit`).
    fn measure_qubit(
        &mut self,
        state: &mut StateVector,
        qubit: usize,
        rng: &mut Mt19937,
    ) -> Result<Measurement> {
        state.measure_qubit(qubit, rng)
    }

    /// Força um qubit a `value` e renormaliza (`StateVector::collapse`).
    fn collapse(&mut self, state: &mut StateVector, qubit: usize, value: u8) -> Result<f64> {
        state.collapse(qubit, value)
    }

    /// Multiplicação modular controlada emulada (`StateVector::apply_mod_mult`).
    fn apply_mod_mult(
        &mut self,
        state: &mut StateVector,
        ctrl_qubit: usize,
        reg_first_qubit: usize,
        width: usize,
        multiplier: i64,
        modulus: i64,
    ) -> Result<()> {
        state.apply_mod_mult(ctrl_qubit, reg_first_qubit, width, multiplier, modulus)
    }

    /// Uma amplitude do estado, sem trazer o vetor inteiro para o host.
    fn amplitude(&mut self, state: &mut StateVector, index: usize) -> Result<C32> {
        state.as_slice().get(index).copied().ok_or_else(|| {
            HgmError::InvalidArgument(format!(
                "índice {index} fora de um estado de {} qubits",
                state.qubits()
            ))
        })
    }
}
