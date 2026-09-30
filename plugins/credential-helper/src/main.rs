#[allow(dead_code)]
#[path = "../../../src-tauri/src/transcription_credentials.rs"]
mod credentials;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: postal-transcription-key PLUGIN_ID PROVIDER_ID".into());
    }
    let key = zeroize::Zeroizing::new(rpassword::prompt_password(format!(
        "API key for {} / {}: ",
        args[0], args[1]
    ))?);
    credentials::set(&args[0], &args[1], key)?;
    println!("Key saved to operating-system credential store.");
    Ok(())
}
