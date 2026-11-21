//! Binário `hgm`. Por enquanto só executa um circuito curto de demonstração no
//! backend serial, para provar que o encanamento parser -> estado -> backend
//! funciona de ponta a ponta. `shor`, `grover` e `general` (com `clap`) entram
//! quando os algoritmos forem portados (`docs/09` §8).

use hgm_engine::{BackendKind, Simulator};

fn main() {
    println!("hgm (esqueleto Rust do HybriD-GM)");
    println!("GPU: {}", hgm_gpu::STATUS);

    // Estado de Bell: H no qubit 0, depois CNOT do qubit 0 para o 1.
    let result = Simulator::new(BackendKind::CpuSerial, 2).and_then(|mut simulator| {
        simulator.run("H,ID;Control1(1),Target1(X)", 1, 1)?;
        Ok(simulator)
    });

    match result {
        Ok(simulator) => {
            println!("backend: {}", simulator.backend_name());
            for (index, amplitude) in simulator.state().as_slice().iter().enumerate() {
                println!("|{index:02b}>  {:+.6} {:+.6}i", amplitude.re, amplitude.im);
            }
        }
        Err(error) => {
            eprintln!("erro: {error}");
            std::process::exit(1);
        }
    }
}
