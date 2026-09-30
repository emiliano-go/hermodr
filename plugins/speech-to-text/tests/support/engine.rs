use std::{fs, path::Path, time::Duration};
fn value(args: &[String], key: &str) -> String {
    args[args.iter().position(|s| s == key).unwrap() + 1].clone()
}
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.iter().any(|s| s == "-i") {
        let input = value(&args, "-i");
        if fs::read(&input).unwrap().ends_with(b"slow") {
            std::thread::sleep(Duration::from_secs(20));
        }
        let output = Path::new(args.last().unwrap());
        let mut wav = b"RIFF".to_vec();
        wav.extend(38u32.to_le_bytes());
        wav.extend(b"WAVEfmt ");
        wav.extend(16u32.to_le_bytes());
        wav.extend(1u16.to_le_bytes());
        wav.extend(1u16.to_le_bytes());
        wav.extend(16000u32.to_le_bytes());
        wav.extend(32000u32.to_le_bytes());
        wav.extend(2u16.to_le_bytes());
        wav.extend(16u16.to_le_bytes());
        wav.extend(b"data");
        wav.extend(2u32.to_le_bytes());
        wav.extend([0, 0]);
        fs::write(output, wav).unwrap();
    } else {
        assert!(args.iter().any(|s| s == "-ng"));
        let output = value(&args, "-of");
        fs::write(format!("{output}.json"),r#"{"result":{"language":"en"},"transcription":[{"text":" synthetic"},{"text":" transcript"}]}"#).unwrap();
    }
}
