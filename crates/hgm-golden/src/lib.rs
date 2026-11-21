//! Leitor dos vetores de referência de `tests/golden` (formato em
//! `tests/golden/README.md`). Só serve a testes: nenhum outro crate de produção
//! depende dele.
//!
//! Erros são `String` de propósito: um teste que não consegue ler o oráculo deve
//! falhar alto, com a mensagem, e este crate não precisa de `thiserror`.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use num_complex::Complex32;

/// Pasta `tests/golden/data` do repositório.
pub fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/golden/data")
}

/// Um caso `.case` já separado em campos.
#[derive(Debug)]
pub struct Case {
    pub path: PathBuf,
    pub name: String,
    pub kind: String,
    /// Linhas `chave valor...` simples (ex.: `qubits 4`, `init file x`, `mode parse`),
    /// na ordem do arquivo. `valor` é o resto da linha.
    pub fields: Vec<(String, String)>,
    /// Linhas `gate NOME m0r m0i ... m3r m3i`.
    pub gates: Vec<(String, [Complex32; 4])>,
    /// Blocos de `N` linhas: `steps`, `pts`, `lines`. Em casos `kind table`, todas
    /// as linhas depois de `kind` ficam em `table`.
    pub blocks: HashMap<String, Vec<String>>,
}

impl Case {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    pub fn get_usize(&self, key: &str) -> Result<usize, String> {
        self.get(key)
            .ok_or_else(|| format!("{}: falta `{key}`", self.name))?
            .trim()
            .parse()
            .map_err(|e| format!("{}: `{key}` inválido: {e}", self.name))
    }

    pub fn block(&self, key: &str) -> &[String] {
        self.blocks.get(key).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Caminho (relativo a `data/`) de um campo `file`/`final file`/`init file`.
    pub fn file_field(&self, key: &str) -> Result<PathBuf, String> {
        let value = self
            .get(key)
            .ok_or_else(|| format!("{}: falta `{key}`", self.name))?;
        let relative = value
            .strip_prefix("file ")
            .ok_or_else(|| format!("{}: `{key} {value}` não é `file <caminho>`", self.name))?;
        Ok(data_dir().join(relative.trim()))
    }
}

/// Lê `INDEX.txt` e devolve o caminho absoluto de cada `.case`.
pub fn load_index() -> Result<Vec<PathBuf>, String> {
    let index_path = data_dir().join("INDEX.txt");
    let text = fs::read_to_string(&index_path)
        .map_err(|e| format!("não consegui ler {}: {e}", index_path.display()))?;

    let mut cases = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("format ") || line.starts_with("cases ") {
            continue;
        }
        cases.push(data_dir().join(line));
    }
    Ok(cases)
}

/// Converte `"1 0 -0 3.99e-09"` em `Vec<f32>`.
pub fn parse_floats(text: &str) -> Result<Vec<f32>, String> {
    text.split_whitespace()
        .map(|token| {
            token
                .parse::<f32>()
                .map_err(|e| format!("número inválido `{token}`: {e}"))
        })
        .collect()
}

pub fn load_case(path: &Path) -> Result<Case, String> {
    let text = fs::read_to_string(path)
        .map_err(|e| format!("não consegui ler {}: {e}", path.display()))?;
    let lines: Vec<&str> = text.lines().collect();

    let mut case = Case {
        path: path.to_owned(),
        name: String::new(),
        kind: String::new(),
        fields: Vec::new(),
        gates: Vec::new(),
        blocks: HashMap::new(),
    };

    let mut index = 0;
    while index < lines.len() {
        let line = lines[index].trim_end();
        index += 1;
        if line.is_empty() {
            continue;
        }
        let (key, rest) = line.split_once(' ').unwrap_or((line, ""));

        match key {
            "name" => case.name = rest.to_owned(),
            "kind" => {
                case.kind = rest.to_owned();
                if case.kind == "table" {
                    let rows = lines[index..]
                        .iter()
                        .map(|l| l.trim_end().to_owned())
                        .filter(|l| !l.is_empty())
                        .collect();
                    case.blocks.insert("table".to_owned(), rows);
                    break;
                }
            }
            "gate" => {
                let (name, numbers) = rest
                    .split_once(' ')
                    .ok_or_else(|| format!("{}: linha `gate` incompleta", path.display()))?;
                let v = parse_floats(numbers)?;
                if v.len() != 8 {
                    return Err(format!(
                        "{}: `gate {name}` precisa de 8 números, tem {}",
                        path.display(),
                        v.len()
                    ));
                }
                let matrix = [
                    Complex32::new(v[0], v[1]),
                    Complex32::new(v[2], v[3]),
                    Complex32::new(v[4], v[5]),
                    Complex32::new(v[6], v[7]),
                ];
                case.gates.push((name.to_owned(), matrix));
            }
            "steps" | "pts" | "lines" => {
                let count: usize = rest
                    .trim()
                    .parse()
                    .map_err(|e| format!("{}: `{key} {rest}` inválido: {e}", path.display()))?;
                if index + count > lines.len() {
                    return Err(format!(
                        "{}: `{key} {count}` mas só há {} linhas depois",
                        path.display(),
                        lines.len() - index
                    ));
                }
                let block = lines[index..index + count]
                    .iter()
                    .map(|l| l.trim_end().to_owned())
                    .collect();
                case.blocks.insert(key.to_owned(), block);
                index += count;
            }
            _ => case.fields.push((key.to_owned(), rest.to_owned())),
        }
    }

    if case.name.is_empty() || case.kind.is_empty() {
        return Err(format!("{}: falta `name` ou `kind`", path.display()));
    }
    Ok(case)
}

/// Lê um `.f32` (float32 little-endian) inteiro.
pub fn read_bytes(path: &Path) -> Result<Vec<u8>, String> {
    fs::read(path).map_err(|e| format!("não consegui ler {}: {e}", path.display()))
}

/// Todos os casos do `INDEX.txt` de um dado `kind`.
pub fn cases_of_kind(kind: &str) -> Result<Vec<Case>, String> {
    let mut cases = Vec::new();
    for path in load_index()? {
        let case = load_case(&path)?;
        if case.kind == kind {
            cases.push(case);
        }
    }
    Ok(cases)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_lists_310_cases() {
        assert_eq!(load_index().unwrap().len(), 310);
    }

    #[test]
    fn every_case_parses() {
        for path in load_index().unwrap() {
            load_case(&path).unwrap();
        }
    }

    #[test]
    fn parses_floats_with_negative_zero() {
        assert_eq!(parse_floats("1 -0 3.99e-09").unwrap().len(), 3);
        assert!(parse_floats("1 x").is_err());
    }
}
