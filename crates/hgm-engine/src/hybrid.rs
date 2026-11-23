//! `t_HYBRID`: a GPU enquanto o estado cabe nela, a CPU paralela quando não cabe.
//!
//! É o que o `t_HYBRID` do C++ virou depois do item 17 de `docs/07`: com o estado na GPU o
//! modo se reduz ao `t_GPU` (ele nunca ganhou do `t_GPU` quando o estado cabe), e a divisão
//! do trabalho entre CPU e GPU só importava quando o estado não cabia. Medido no Rust
//! (`docs/09`, fase 6): sem memória fixada, o wgpu transfere a ~2,1 GB/s para a GPU e
//! ~2,3 GB/s de volta, então levar blocos do estado à GPU e trazê-los de volta só compensa em
//! lotes grandes de portas; este módulo não faz isso (ainda): sem espaço na GPU, tudo roda na
//! CPU paralela.
//!
//! O backend escolhe pelo tamanho (`gpu_max_qubits`), pela existência de adaptador e,
//! durante a execução, pela falta de memória da GPU ([`HgmError::DeviceOutOfMemory`]): nesse
//! caso traz o estado para o host e segue na CPU. O recuo é de mão única (o estado não volta
//! para a GPU sozinho).

use hgm_core::error::{HgmError, Result};
use hgm_core::{Backend, Measurement, Mt19937, Program, StateVector, C32};

/// Onde o estado está.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HybridMode {
    /// O estado mora na GPU (backend residente).
    Gpu,
    /// O estado mora no host e a CPU paralela o processa.
    Cpu,
}

pub struct HybridBackend {
    /// `None` se não há GPU (sem adaptador ou estado grande demais para ela).
    gpu: Option<Box<dyn Backend>>,
    cpu: Box<dyn Backend>,
    mode: HybridMode,
    name: String,
}

impl HybridBackend {
    /// `gpu` é o backend residente (se há e se o estado cabe nele); `why_not_gpu` diz por
    /// que não há, para o nome do backend.
    pub fn new(
        gpu: Option<Box<dyn Backend>>,
        cpu: Box<dyn Backend>,
        why_not_gpu: Option<String>,
    ) -> Self {
        let mut backend = Self {
            mode: if gpu.is_some() {
                HybridMode::Gpu
            } else {
                HybridMode::Cpu
            },
            gpu,
            cpu,
            name: String::new(),
        };
        backend.rename(why_not_gpu.as_deref());
        backend
    }

    pub fn mode(&self) -> HybridMode {
        self.mode
    }

    fn rename(&mut self, why_not_gpu: Option<&str>) {
        self.name = match (self.mode, why_not_gpu) {
            (HybridMode::Gpu, _) => "t_HYBRID (Rust, estado na GPU)".to_owned(),
            (HybridMode::Cpu, Some(why)) => format!("t_HYBRID (Rust, CPU paralela: {why})"),
            (HybridMode::Cpu, None) => "t_HYBRID (Rust, CPU paralela)".to_owned(),
        };
    }

    /// Passa o estado da GPU para o host e segue na CPU.
    fn fall_back(&mut self, state: &mut StateVector, reason: &str) -> Result<()> {
        if let Some(gpu) = self.gpu.as_mut() {
            gpu.sync_to_host(state)?;
            gpu.host_state_replaced();
        }
        self.gpu = None;
        self.mode = HybridMode::Cpu;
        self.rename(Some(&format!("a GPU ficou sem memória ({reason})")));
        Ok(())
    }

    /// Roda `op` no backend atual; se a GPU ficar sem memória, recua para a CPU e repete.
    /// A GPU só pede memória antes de mexer no estado, então repetir é seguro.
    fn run<T>(
        &mut self,
        state: &mut StateVector,
        mut op: impl FnMut(&mut dyn Backend, &mut StateVector) -> Result<T>,
    ) -> Result<T> {
        if let Some(gpu) = self.gpu.as_mut() {
            match op(gpu.as_mut(), state) {
                Err(HgmError::DeviceOutOfMemory(reason)) => self.fall_back(state, &reason)?,
                other => return other,
            }
        }
        op(self.cpu.as_mut(), state)
    }
}

impl Backend for HybridBackend {
    fn name(&self) -> &str {
        &self.name
    }

    fn execute(
        &mut self,
        state: &mut StateVector,
        program: &Program,
        repetitions: u32,
    ) -> Result<()> {
        self.run(state, |backend, state| {
            backend.execute(state, program, repetitions)
        })
    }

    fn initial_state_ready(&mut self, state: &mut StateVector) -> Result<()> {
        self.run(state, |backend, state| backend.initial_state_ready(state))
    }

    fn host_state_replaced(&mut self) {
        if let Some(gpu) = self.gpu.as_mut() {
            gpu.host_state_replaced();
        }
        self.cpu.host_state_replaced();
    }

    fn set_basis(&mut self, state: &mut StateVector, index: usize) -> Result<()> {
        self.run(state, |backend, state| backend.set_basis(state, index))
    }

    fn sync_to_host(&mut self, state: &mut StateVector) -> Result<()> {
        match self.gpu.as_mut() {
            Some(gpu) => gpu.sync_to_host(state),
            None => Ok(()),
        }
    }

    fn measure_qubit(
        &mut self,
        state: &mut StateVector,
        qubit: usize,
        rng: &mut Mt19937,
    ) -> Result<Measurement> {
        self.run(state, |backend, state| {
            backend.measure_qubit(state, qubit, rng)
        })
    }

    fn collapse(&mut self, state: &mut StateVector, qubit: usize, value: u8) -> Result<f64> {
        self.run(state, |backend, state| {
            backend.collapse(state, qubit, value)
        })
    }

    fn apply_mod_mult(
        &mut self,
        state: &mut StateVector,
        ctrl_qubit: usize,
        reg_first_qubit: usize,
        width: usize,
        multiplier: i64,
        modulus: i64,
    ) -> Result<()> {
        self.run(state, |backend, state| {
            backend.apply_mod_mult(
                state,
                ctrl_qubit,
                reg_first_qubit,
                width,
                multiplier,
                modulus,
            )
        })
    }

    fn amplitude(&mut self, state: &mut StateVector, index: usize) -> Result<C32> {
        self.run(state, |backend, state| backend.amplitude(state, index))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hgm_core::circuit::compile_steps;
    use hgm_core::GateCatalog;
    use hgm_cpu::SerialBackend;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// "GPU" de mentira: roda no host (como um backend serial) e fica sem memória quando o
    /// teste manda. Registra o que o híbrido pediu.
    struct FakeGpu {
        inner: SerialBackend,
        log: Rc<RefCell<Vec<&'static str>>>,
        out_of_memory_on_mod_mult: bool,
        out_of_memory_on_first_use: bool,
    }

    impl Backend for FakeGpu {
        fn name(&self) -> &str {
            "falsa"
        }

        fn execute(&mut self, state: &mut StateVector, program: &Program, n: u32) -> Result<()> {
            if self.out_of_memory_on_first_use {
                self.log.borrow_mut().push("gpu: sem memória");
                return Err(HgmError::DeviceOutOfMemory("estado".to_owned()));
            }
            self.log.borrow_mut().push("gpu: execute");
            self.inner.execute(state, program, n)
        }

        fn sync_to_host(&mut self, _state: &mut StateVector) -> Result<()> {
            self.log.borrow_mut().push("gpu: sync");
            Ok(())
        }

        fn host_state_replaced(&mut self) {
            self.log.borrow_mut().push("gpu: invalidada");
        }

        fn apply_mod_mult(
            &mut self,
            _state: &mut StateVector,
            _ctrl_qubit: usize,
            _reg_first_qubit: usize,
            _width: usize,
            _multiplier: i64,
            _modulus: i64,
        ) -> Result<()> {
            if self.out_of_memory_on_mod_mult {
                self.log.borrow_mut().push("gpu: sem memória");
                return Err(HgmError::DeviceOutOfMemory("blocos auxiliares".to_owned()));
            }
            Ok(())
        }
    }

    fn program(qubits: usize) -> Program {
        let steps: Vec<String> = (0..qubits)
            .map(|q| {
                let mut tokens = vec!["ID"; qubits];
                tokens[q] = "H";
                tokens.join(",")
            })
            .collect();
        compile_steps(&steps, 1, &GateCatalog::with_base_gates()).unwrap()
    }

    fn hybrid(
        log: &Rc<RefCell<Vec<&'static str>>>,
        mod_mult: bool,
        first_use: bool,
    ) -> HybridBackend {
        HybridBackend::new(
            Some(Box::new(FakeGpu {
                inner: SerialBackend::new(),
                log: Rc::clone(log),
                out_of_memory_on_mod_mult: mod_mult,
                out_of_memory_on_first_use: first_use,
            })),
            Box::new(SerialBackend::new()),
            None,
        )
    }

    #[test]
    fn without_a_gpu_everything_runs_on_the_cpu_backend() {
        let mut backend = HybridBackend::new(
            None,
            Box::new(SerialBackend::new()),
            Some("sem adaptador".to_owned()),
        );
        assert_eq!(backend.mode(), HybridMode::Cpu);
        assert!(backend.name().contains("sem adaptador"));
        let mut state = StateVector::basis(4, 0).unwrap();
        backend.execute(&mut state, &program(4), 1).unwrap();
        assert!((state.norm_sqr() - 1.0).abs() < 1e-6);
        backend.sync_to_host(&mut state).unwrap();
    }

    #[test]
    fn running_out_of_gpu_memory_before_touching_the_state_falls_back_to_the_cpu() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut backend = hybrid(&log, false, true);
        assert_eq!(backend.mode(), HybridMode::Gpu);
        let mut state = StateVector::basis(4, 0).unwrap();
        backend.execute(&mut state, &program(4), 1).unwrap();
        assert_eq!(backend.mode(), HybridMode::Cpu);
        assert!(backend.name().contains("sem memória"));
        // O resultado é o da CPU.
        let mut want = StateVector::basis(4, 0).unwrap();
        SerialBackend::new()
            .execute(&mut want, &program(4), 1)
            .unwrap();
        assert!(state.max_abs_diff(&want) < 1e-7);
        // O recuo trouxe o estado da GPU (que não existia) e a invalidou, uma vez.
        assert_eq!(
            *log.borrow(),
            vec!["gpu: sem memória", "gpu: sync", "gpu: invalidada"]
        );
        // Depois do recuo a GPU não é mais consultada.
        backend.execute(&mut state, &program(4), 1).unwrap();
        assert_eq!(log.borrow().len(), 3);
    }

    #[test]
    fn running_out_of_gpu_memory_in_mod_mult_brings_the_state_home_first() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut backend = hybrid(&log, true, false);
        let mut state = StateVector::basis(7, 0).unwrap();
        backend.execute(&mut state, &program(7), 1).unwrap();
        backend.apply_mod_mult(&mut state, 0, 1, 4, 7, 15).unwrap();
        assert_eq!(backend.mode(), HybridMode::Cpu);

        let mut want = StateVector::basis(7, 0).unwrap();
        SerialBackend::new()
            .execute(&mut want, &program(7), 1)
            .unwrap();
        want.apply_mod_mult(0, 1, 4, 7, 15).unwrap();
        assert!(state.max_abs_diff(&want) < 1e-7);
        // Primeiro a GPU executou, depois falhou, e só então o estado foi trazido.
        assert_eq!(
            *log.borrow(),
            vec![
                "gpu: execute",
                "gpu: sem memória",
                "gpu: sync",
                "gpu: invalidada"
            ]
        );
    }

    #[test]
    fn other_gpu_errors_are_not_swallowed() {
        struct Broken;
        impl Backend for Broken {
            fn name(&self) -> &str {
                "quebrada"
            }
            fn execute(&mut self, _: &mut StateVector, _: &Program, _: u32) -> Result<()> {
                Err(HgmError::Backend("driver".to_owned()))
            }
        }
        let mut backend =
            HybridBackend::new(Some(Box::new(Broken)), Box::new(SerialBackend::new()), None);
        let mut state = StateVector::basis(2, 0).unwrap();
        assert!(matches!(
            backend.execute(&mut state, &program(2), 1),
            Err(HgmError::Backend(_))
        ));
        assert_eq!(backend.mode(), HybridMode::Gpu);
    }
}
