//! `speedyspec-sql-corpus ingest <file.jsonl>...`: merges collected statements into `compat/sql-corpus/corpus.json`.

use std::collections::BTreeMap;
use std::io::BufReader;
use std::process::ExitCode;

use speedyspec_data_corpus::{CorpusEntry, corpus_dir, ingest, load_corpus};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(("ingest", files)) = args.split_first().map(|(c, f)| (c.as_str(), f)) else {
        eprintln!("usage: speedyspec-sql-corpus ingest <file.jsonl>...");
        return ExitCode::from(2);
    };
    let mut corpus: BTreeMap<String, CorpusEntry> = load_corpus()
        .into_iter()
        .map(|e| (e.shape.clone().unwrap_or_else(|| e.example.clone()), e))
        .collect();
    for file in files {
        match std::fs::File::open(file) {
            Ok(f) => ingest(BufReader::new(f), &mut corpus),
            Err(e) => {
                eprintln!("{file}: {e}");
                return ExitCode::FAILURE;
            }
        }
    }
    let entries: Vec<&CorpusEntry> = corpus.values().collect();
    let path = corpus_dir().join("corpus.json");
    let text = serde_json::to_string_pretty(&entries).expect("entries serialize");
    if let Err(e) =
        std::fs::create_dir_all(corpus_dir()).and_then(|()| std::fs::write(&path, text + "\n"))
    {
        eprintln!("{}: {e}", path.display());
        return ExitCode::FAILURE;
    }
    println!("{} shapes in {}", entries.len(), path.display());
    ExitCode::SUCCESS
}
